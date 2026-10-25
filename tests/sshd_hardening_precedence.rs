//! sshd hardening wins over provider and cloud-init drop-ins.
//!
//! sshd keeps the first value it reads for a keyword, and `Include` splices
//! the drop-ins in place. A drop-in read before the hardening directives
//! therefore decides them. The rendered config must read every hardening
//! directive before the Include, and the deploy must check what sshd
//! actually resolved. `sshd -T` prints global values only, so the deploy
//! also refuses drop-ins that open a `Match` block.

use std::collections::{BTreeMap, HashMap};
use std::fs;

use serde_yaml::Value;

mod common;

use serde_yaml::Mapping;

use common::{Task, field, role_dir, role_tasks, strings};

const DROP_IN_DIR: &str = "/etc/ssh/sshd_config.d";
const ALIAS_SSHD_T_OMITS: &str = "challengeresponseauthentication";

const HOSTILE_DROP_IN: &str = "\
PasswordAuthentication yes
KbdInteractiveAuthentication yes
ChallengeResponseAuthentication yes
PermitEmptyPasswords yes
PermitRootLogin yes
PubkeyAuthentication no
Match all
";

const HARDENED: [(&str, &str); 6] = [
    ("passwordauthentication", "no"),
    ("kbdinteractiveauthentication", "no"),
    ("challengeresponseauthentication", "no"),
    ("permitemptypasswords", "no"),
    ("permitrootlogin", "no"),
    ("pubkeyauthentication", "yes"),
];

fn render_sshd_config() -> String {
    let path = role_dir("ssh").join("templates/sshd_config.j2");
    let template = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let ctx: HashMap<&str, &str> = [
        "ssh_port",
        "ssh_max_auth_tries",
        "ssh_max_sessions",
        "ssh_login_grace_time",
        "ssh_log_level",
        "ssh_client_alive_interval",
        "ssh_client_alive_count_max",
    ]
    .into_iter()
    .map(|key| (key, "1"))
    .collect();
    let mut env = minijinja::Environment::new();
    env.set_trim_blocks(true);
    env.render_str(&template, ctx)
        .expect("sshd_config.j2 must render")
}

fn effective_global_config(main: &str, drop_in: &str) -> BTreeMap<String, String> {
    let mut effective = BTreeMap::new();
    let lines = main.lines().flat_map(|line| {
        if line.trim_start().to_lowercase().starts_with("include ") {
            drop_in.lines().collect::<Vec<_>>()
        } else {
            vec![line]
        }
    });
    for line in lines {
        let mut words = line.split_whitespace();
        let Some(keyword) = words.next().filter(|w| !w.starts_with('#')) else {
            continue;
        };
        let keyword = keyword.to_lowercase();
        if keyword == "match" {
            break;
        }
        let value = words.next().unwrap_or_default().to_string();
        effective.entry(keyword).or_insert(value);
    }
    effective
}

#[test]
fn test_the_rendered_config_still_includes_the_drop_ins() {
    let rendered = render_sshd_config();
    assert!(
        rendered
            .lines()
            .any(|line| line.trim() == "Include /etc/ssh/sshd_config.d/*.conf"),
        "sshd_config must keep including /etc/ssh/sshd_config.d"
    );
}

#[test]
fn test_hardening_wins_over_a_hostile_drop_in() {
    let effective = effective_global_config(&render_sshd_config(), HOSTILE_DROP_IN);
    for (keyword, expected) in HARDENED {
        assert_eq!(
            effective.get(keyword).map(String::as_str),
            Some(expected),
            "a drop-in must not override `{keyword}`"
        );
    }
}

fn assert_conditions(tasks: &[Task]) -> Vec<String> {
    tasks
        .iter()
        .filter_map(|task| field(&task.body, "ansible.builtin.assert"))
        .flat_map(|assert| strings(assert.as_mapping().and_then(|m| field(m, "that"))))
        .collect()
}

fn registered_by<'a>(
    tasks: &'a [Task],
    module: &str,
    matches: impl Fn(&Mapping) -> bool,
) -> &'a str {
    tasks
        .iter()
        .find(|task| {
            field(&task.body, module)
                .and_then(Value::as_mapping)
                .is_some_and(&matches)
        })
        .and_then(|task| field(&task.body, "register"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the ssh role must register a matching `{module}` task"))
}

#[test]
fn test_the_deploy_asserts_the_effective_auth_settings() {
    let tasks = role_tasks("ssh");
    let register = registered_by(&tasks, "ansible.builtin.command", |module| {
        field(module, "cmd")
            .and_then(Value::as_str)
            .is_some_and(|cmd| cmd.contains("sshd -T"))
    });
    let conditions = assert_conditions(&tasks);
    for (keyword, value) in HARDENED
        .into_iter()
        .filter(|(k, _)| *k != ALIAS_SSHD_T_OMITS)
    {
        let expected = format!("'{keyword} {value}' in {register}.stdout_lines");
        assert!(
            conditions.contains(&expected),
            "the ssh role must assert `{expected}`"
        );
    }
}

#[test]
fn test_the_deploy_refuses_match_blocks_in_drop_ins() {
    let tasks = role_tasks("ssh");
    let register = registered_by(&tasks, "ansible.builtin.find", |module| {
        field(module, "paths").and_then(Value::as_str) == Some(DROP_IN_DIR)
            && field(module, "contains")
                .and_then(Value::as_str)
                .is_some_and(|pattern| pattern.contains("match"))
    });
    let expected = format!("{register}.matched == 0");
    assert!(
        assert_conditions(&tasks).contains(&expected),
        "the ssh role must assert `{expected}`: sshd -T cannot see Match-scoped overrides"
    );
}
