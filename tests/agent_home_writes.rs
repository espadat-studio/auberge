//! The agent tier writes its own home as the agent, never as root.
//!
//! The agent user owns its home and every directory in it, so it can swap any
//! path component for a link between two runs. A root task that writes or
//! re-owns that path then lands wherever the link points. Writing as the agent
//! keeps a swapped path inside the agent's own permissions, and
//! `follow: false` on the directories fails the run on a link instead of
//! descending through it.
//!
//! - every task whose `path`/`dest` sits under the role's home runs with
//!   `become_user` set to the role's user;
//! - none of them names an `owner` or `group`: the writer is the owner;
//! - every directory among them refuses to follow a link.

use serde_yaml::{Mapping, Value};

mod common;

use common::{defaults, field, resolve, role_tasks, task_name};

/// Each agent-tier role writing under the agent home, with its user key, its
/// home key, and how many of its tasks write there.
const AGENT_HOME_ROLES: &[(&str, &str, &str, usize)] = &[
    ("opencode", "opencode_user", "opencode_home", 2),
    ("aoe", "aoe_user", "aoe_home", 4),
];

/// The module argument mappings of a task that name a filesystem target.
fn targets(task: &Mapping) -> Vec<(&Mapping, String)> {
    task.values()
        .filter_map(Value::as_mapping)
        .filter_map(|args| {
            ["path", "dest", "name"]
                .iter()
                .find_map(|key| field(args, key).and_then(Value::as_str))
                .map(|target| (args, target.to_string()))
        })
        .collect()
}

#[test]
fn test_agent_home_tasks_write_as_the_agent_and_refuse_links() {
    for (role, user_key, home_key, expected) in AGENT_HOME_ROLES {
        let vars = defaults(role);
        let home = resolve(&format!("{{{{ {home_key} }}}}"), &vars);
        assert!(
            home.starts_with("/home/"),
            "{role}: `{home_key}` resolved to `{home}`; the fence would match nothing"
        );
        let user = resolve(&format!("{{{{ {user_key} }}}}"), &vars);

        let mut seen = 0;
        for task in role_tasks(role) {
            for (args, target) in targets(&task.body) {
                if !resolve(&target, &vars).starts_with(&format!("{home}/")) {
                    continue;
                }
                seen += 1;
                let name = task_name(&task.body);
                let become_user = field(&task.body, "become_user")
                    .and_then(Value::as_str)
                    .map(|value| resolve(value, &vars));
                assert_eq!(
                    become_user.as_deref(),
                    Some(user.as_str()),
                    "{role}: `{name}` writes {target} as root; it must run as `{user_key}`"
                );
                for key in ["owner", "group"] {
                    assert!(
                        field(args, key).is_none(),
                        "{role}: `{name}` sets `{key}`; the agent writing it is its owner"
                    );
                }
                if field(args, "state").and_then(Value::as_str) == Some("directory") {
                    assert_eq!(
                        field(args, "follow").and_then(Value::as_bool),
                        Some(false),
                        "{role}: `{name}` would descend through a link at {target}"
                    );
                }
            }
        }
        assert_eq!(
            seen, *expected,
            "{role}: {seen} tasks write under {home}, the fence declares {expected}"
        );
    }
}
