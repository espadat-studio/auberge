//! Paperless's redis answers only to paperless.
//!
//! redis is the Celery broker: a client that reaches it reads, flushes or
//! injects the worker's tasks. Debian's redis listens on loopback TCP with no
//! auth, so every uid on the Host is such a client, and a Host co-locates
//! Apps. So redis drops TCP and listens on a socket only its group may open,
//! and the paperless service user is the one member that role adds:
//!
//! - the override the role ships turns TCP off and opens a group-only socket;
//! - the override is spliced in as the last line of `redis.conf`, since redis
//!   keeps the last value it reads for a directive;
//! - paperless points at that same socket, and its user joins the group;
//! - redis restarts only after both, since every paperless unit `Requires=`
//!   it and restarts with it: earlier, they come back dialing a closed port;
//! - the deploy probes the result: paperless gets PONG, an outsider uid and
//!   loopback TCP do not.

use std::collections::BTreeMap;
use std::fs;

use serde_yaml::{Mapping, Value};

mod common;

use common::{Plays, field, role_dir, role_tasks, strings, task_name, tasks_in};

const OVERRIDE_SRC: &str = "redis-socket.conf";
const OVERRIDE_DEST: &str = "/etc/redis/redis-socket.conf";
const REDIS_CONF: &str = "/etc/redis/redis.conf";
const REDIS_GROUP: &str = "redis";

fn override_directives() -> BTreeMap<String, String> {
    let path = role_dir("paperless").join("files").join(OVERRIDE_SRC);
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once(char::is_whitespace))
        .map(|(key, value)| (key.to_lowercase(), value.trim().to_string()))
        .collect()
}

fn module<'a>(task: &'a Mapping, name: &str) -> Option<&'a Mapping> {
    field(task, name).and_then(Value::as_mapping)
}

fn str_at<'a>(args: &'a Mapping, key: &str) -> Option<&'a str> {
    field(args, key).and_then(Value::as_str)
}

#[test]
fn test_the_override_disables_tcp_and_opens_a_group_only_socket() {
    let directives = override_directives();
    assert_eq!(
        directives.get("port").map(String::as_str),
        Some("0"),
        "redis must not listen on TCP"
    );
    assert!(
        directives
            .get("unixsocket")
            .is_some_and(|p| p.starts_with('/')),
        "redis must listen on an absolute socket path"
    );
    let perm = directives
        .get("unixsocketperm")
        .expect("the socket must declare its mode");
    let mode = u32::from_str_radix(perm, 8).unwrap_or_else(|e| panic!("{perm}: {e}"));
    assert_eq!(
        mode & 0o007,
        0,
        "other uids must not open the socket, got {perm}"
    );
    assert_eq!(
        mode & 0o060,
        0o060,
        "the redis group must open the socket, got {perm}"
    );
}

#[test]
fn test_the_override_is_the_last_word_in_redis_conf() {
    let tasks = role_tasks("paperless");

    let shipped = tasks
        .iter()
        .filter_map(|t| module(&t.body, "ansible.builtin.copy"))
        .any(|copy| {
            str_at(copy, "src") == Some(OVERRIDE_SRC)
                && str_at(copy, "dest") == Some(OVERRIDE_DEST)
                && str_at(copy, "group") == Some(REDIS_GROUP)
        });
    assert!(
        shipped,
        "the role must copy {OVERRIDE_SRC} to {OVERRIDE_DEST}, readable by {REDIS_GROUP}"
    );

    let include = format!("include {OVERRIDE_DEST}");
    let spliced = tasks
        .iter()
        .filter_map(|t| module(&t.body, "ansible.builtin.lineinfile"))
        .any(|edit| {
            str_at(edit, "path") == Some(REDIS_CONF)
                && str_at(edit, "line") == Some(include.as_str())
                && str_at(edit, "insertafter") == Some("EOF")
        });
    assert!(spliced, "{REDIS_CONF} must end with `{include}`");
}

#[test]
fn test_paperless_reaches_redis_through_the_socket_as_a_group_member() {
    let socket = override_directives()
        .remove("unixsocket")
        .expect("the override must declare its socket");

    let path = role_dir("paperless").join("templates/paperless.conf.j2");
    let conf = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let urls: Vec<_> = conf
        .lines()
        .filter_map(|line| line.strip_prefix("PAPERLESS_REDIS="))
        .collect();
    assert_eq!(
        urls,
        [format!("unix://{socket}")],
        "paperless must dial the redis socket"
    );

    let joined = role_tasks("paperless")
        .iter()
        .filter_map(|t| module(&t.body, "ansible.builtin.user"))
        .filter(|user| str_at(user, "name") == Some("{{ paperless_sys_user }}"))
        .any(|user| {
            strings(field(user, "groups")).contains(&REDIS_GROUP.to_string())
                && field(user, "append").and_then(Value::as_bool) == Some(true)
        });
    assert!(
        joined,
        "the paperless user must join `{REDIS_GROUP}` without dropping other groups"
    );
}

fn position(tasks: &[common::Task], what: &str, matches: impl Fn(&Mapping) -> bool) -> usize {
    tasks
        .iter()
        .position(|t| matches(&t.body))
        .unwrap_or_else(|| panic!("tasks/main.yml must {what}"))
}

#[test]
fn test_redis_restarts_onto_the_socket_after_paperless_is_ready_for_it() {
    let tasks = tasks_in(
        &role_dir("paperless").join("tasks/main.yml"),
        Plays::AsTasks,
    );
    let restart = position(&tasks, "restart redis-server", |body| {
        module(body, "ansible.builtin.systemd_service").is_some_and(|svc| {
            str_at(svc, "name") == Some("redis-server") && str_at(svc, "state") == Some("restarted")
        })
    });
    let joined = position(&tasks, "create the paperless user", |body| {
        module(body, "ansible.builtin.user").is_some()
    });
    let conf = position(&tasks, "deploy paperless.conf", |body| {
        module(body, "ansible.builtin.template")
            .is_some_and(|tpl| str_at(tpl, "src") == Some("paperless.conf.j2"))
    });
    assert!(
        joined < restart && conf < restart,
        "`{}` must run after the paperless user joins `{REDIS_GROUP}` and paperless.conf names the socket",
        task_name(&tasks[restart].body)
    );
}

#[test]
fn test_the_deploy_probes_who_redis_answers() {
    let socket = override_directives()
        .remove("unixsocket")
        .expect("the override must declare its socket");
    let probes: Vec<(Option<String>, String)> = role_tasks("paperless")
        .iter()
        .filter_map(|t| {
            let cmd = field(&t.body, "ansible.builtin.command")?.as_str()?;
            cmd.starts_with("redis-cli").then(|| {
                let user = field(&t.body, "become_user")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                (user, cmd.to_string())
            })
        })
        .collect();
    let probed = |user: Option<&str>, target: &str| {
        probes
            .iter()
            .any(|(u, cmd)| u.as_deref() == user && cmd.contains(target))
    };
    assert!(
        probed(Some("{{ paperless_sys_user }}"), &socket),
        "the deploy must probe the socket as paperless"
    );
    assert!(
        probed(Some("nobody"), &socket),
        "the deploy must probe the socket as a uid outside `{REDIS_GROUP}`"
    );
    assert!(
        probed(None, "-p 6379"),
        "the deploy must probe loopback TCP"
    );
}
