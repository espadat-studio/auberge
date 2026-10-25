//! The agent tier writes its own home as the agent, never as root.
//!
//! The agent user owns its home and every directory in it, so it can swap any
//! path component for a link between two runs. A root task that writes or
//! re-owns that path then lands wherever the link points. Writing as the agent
//! keeps a swapped path inside the agent's own permissions, and
//! `follow: false` on the directories fails the run on a link instead of
//! descending through it.
//!
//! - every task whose `path`/`dest` sits under the agent's home runs with
//!   `become_user` set to the agent user;
//! - none of them names an `owner` or `group`: the writer is the owner;
//! - every directory among them refuses to follow a link;
//! - a directory root still has to hand to the agent, because its group is one
//!   the agent cannot chown to, refuses to follow a link too.
//!
//! Each role is read on the agent host's terms: its defaults under
//! `group_vars/agent.yml`, which is where memsearch and syncthing learn that
//! their user is the agent.

use std::collections::BTreeMap;

use serde_yaml::{Mapping, Value};

mod common;

use common::{defaults, field, parse_yaml, repo, resolve, role_tasks, task_name};

/// Every role the agent host runs, with how many of its tasks write under the
/// agent's home.
const AGENT_HOST_ROLES: &[(&str, usize)] = &[
    ("opencode", 2),
    ("aoe", 4),
    ("memsearch", 2),
    ("syncthing", 0),
    ("github_identity", 0),
];

/// Root-run directories handed to the agent across [`AGENT_HOST_ROLES`]: the
/// memsearch store root and memory directory, and the syncthing workspace.
const ROOT_DIRECTORIES_HANDED_TO_THE_AGENT: usize = 3;

const AGENT_USER: &str = "{{ agent_user_name }}";

/// A role's defaults with the agent group's bindings over them.
fn agent_host_vars(role: &str) -> BTreeMap<String, String> {
    let mut vars = defaults(role);
    let overlay = parse_yaml(&repo().join("ansible/group_vars/agent.yml"));
    for (key, value) in overlay
        .as_mapping()
        .expect("group_vars/agent.yml must hold a mapping")
    {
        if let (Some(key), Some(value)) = (key.as_str(), value.as_str()) {
            vars.insert(key.to_string(), value.to_string());
        }
    }
    vars
}

/// The module arguments of a task that names a filesystem target.
fn target(task: &Mapping) -> Option<(&Mapping, String)> {
    task.iter()
        .filter(|(key, _)| {
            key.as_str()
                .is_some_and(|key| key.starts_with("ansible.builtin."))
        })
        .find_map(|(_, args)| {
            let args = args.as_mapping()?;
            ["path", "dest", "name"]
                .iter()
                .find_map(|key| field(args, key).and_then(Value::as_str))
                .map(|target| (args, target.to_string()))
        })
}

fn runs_as(task: &Mapping, vars: &BTreeMap<String, String>) -> Option<String> {
    field(task, "become_user")
        .and_then(Value::as_str)
        .map(|value| resolve(value, vars))
}

fn is_directory(args: &Mapping) -> bool {
    field(args, "state").and_then(Value::as_str) == Some("directory")
}

fn refuses_links(args: &Mapping) -> bool {
    field(args, "follow").and_then(Value::as_bool) == Some(false)
}

#[test]
fn test_agent_home_tasks_write_as_the_agent_and_refuse_links() {
    for (role, expected) in AGENT_HOST_ROLES {
        let vars = agent_host_vars(role);
        let home = format!("/home/{AGENT_USER}");
        let mut seen = 0;
        for task in role_tasks(role) {
            let Some((args, target)) = target(&task.body) else {
                continue;
            };
            if !resolve(&target, &vars).starts_with(&format!("{home}/")) {
                continue;
            }
            seen += 1;
            let name = task_name(&task.body);
            assert_eq!(
                runs_as(&task.body, &vars).as_deref(),
                Some(AGENT_USER),
                "{role}: `{name}` writes {target} as root; it must run as `{role}_user`"
            );
            for key in ["owner", "group"] {
                assert!(
                    field(args, key).is_none(),
                    "{role}: `{name}` sets `{key}`; the agent writing it is its owner"
                );
            }
            if is_directory(args) {
                assert!(
                    refuses_links(args),
                    "{role}: `{name}` would descend through a link at {target}"
                );
            }
        }
        assert_eq!(
            seen, *expected,
            "{role}: {seen} tasks write under {home}, the fence declares {expected}"
        );
    }
}

#[test]
fn test_root_directories_handed_to_the_agent_refuse_links() {
    let mut seen = 0;
    for (role, _) in AGENT_HOST_ROLES {
        let vars = agent_host_vars(role);
        for task in role_tasks(role) {
            let Some((args, target)) = target(&task.body) else {
                continue;
            };
            let owner = field(args, "owner").and_then(Value::as_str);
            if runs_as(&task.body, &vars).is_some()
                || !is_directory(args)
                || owner.map(|owner| resolve(owner, &vars)).as_deref() != Some(AGENT_USER)
            {
                continue;
            }
            seen += 1;
            assert!(
                refuses_links(args),
                "{role}: `{}` re-owns {target} as root through any link the agent plants there",
                task_name(&task.body)
            );
        }
    }
    assert_eq!(seen, ROOT_DIRECTORIES_HANDED_TO_THE_AGENT);
}
