//! The agent tier runs unprivileged, and the unprivilege is a declaration this
//! fence holds in place.
//!
//! ADR-0065 once leaned on a sudoers password the agents did not hold; the
//! admin user in fact has passwordless sudo, so the claim was false. The agent
//! tier now runs as its own user from the `agent_users` role, and four things
//! have to stay true for that to mean anything:
//!
//! - no agent-tier role falls back to `admin_user_name` for its runtime user;
//! - the `agent_users` role never grants the agent user `sudo`, `docker`,
//!   `adm`, or a sudoers entry;
//! - the syncthing user changes for the `agent` group alone, from
//!   `group_vars/agent.yml`, never from the role's own default;
//! - the memory store is `agent:syncthing`, mode `2770`, on both ends of the
//!   run that creates it.

use serde_yaml::Value;

mod common;

use common::{defaults, field, parse_yaml, playbooks_dir, repo, role_dir, role_tasks};

const AGENT_ROLES: &[(&str, &str)] = &[
    ("aoe", "aoe_user"),
    ("opencode", "opencode_user"),
    ("github_identity", "github_identity_user"),
];

const PRIVILEGED_GROUPS: &[&str] = &["sudo", "docker", "adm", "wheel", "root"];

fn agent_group_vars() -> serde_yaml::Mapping {
    parse_yaml(&repo().join("ansible/group_vars/agent.yml"))
        .as_mapping()
        .expect("group_vars/agent.yml must hold a mapping")
        .clone()
}

fn text(map: &serde_yaml::Mapping, key: &str) -> String {
    field(map, key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("group_vars/agent.yml must bind `{key}`"))
        .to_string()
}

#[test]
fn test_no_agent_role_defaults_its_user_to_the_admin_user() {
    for (role, key) in AGENT_ROLES {
        let vars = defaults(role);
        let value = vars
            .get(*key)
            .unwrap_or_else(|| panic!("{role} defaults must declare `{key}`"));
        assert!(
            !value.contains("admin_user_name"),
            "{role}: `{key}` is `{value}`; the agent tier runs as `agent_user_name`"
        );
        assert!(
            value.contains("agent_user_name"),
            "{role}: `{key}` is `{value}`; it must read the one shared agent user"
        );
    }
}

#[test]
fn test_memsearch_runs_as_the_agent_user_on_the_agent_group() {
    let vars = agent_group_vars();
    assert!(text(&vars, "memsearch_user").contains("agent_user_name"));
}

#[test]
fn test_agent_users_grants_no_privileged_group_and_no_sudoers_entry() {
    let defaults_path = role_dir("agent_users").join("defaults/main.yml");
    let granted = common::strings(parse_yaml(&defaults_path).get("agent_users_groups"));
    for group in PRIVILEGED_GROUPS {
        assert!(
            !granted.iter().any(|g| g == group),
            "agent_users_groups grants `{group}`, which is root-equivalent or reads /var/log"
        );
    }

    for task in role_tasks("agent_users") {
        let body = serde_yaml::to_string(&task.body).unwrap();
        assert!(
            !body.contains("sudoers"),
            "agent_users task `{}` touches sudoers",
            common::task_name(&task.body)
        );
        for group in PRIVILEGED_GROUPS {
            assert!(
                !body.contains(&format!("[{group}]")) && !body.contains(&format!("- {group}\n")),
                "agent_users task `{}` names the `{group}` group",
                common::task_name(&task.body)
            );
        }
    }
}

#[test]
fn test_the_agent_user_reads_the_journal() {
    let path = role_dir("agent_users").join("defaults/main.yml");
    let granted = common::strings(parse_yaml(&path).get("agent_users_groups"));
    assert!(granted.iter().any(|g| g == "systemd-journal"));
}

#[test]
fn test_syncthing_user_changes_for_the_agent_group_only() {
    let role_default = parse_yaml(&role_dir("syncthing").join("defaults/main.yml"));
    let role_default = field(role_default.as_mapping().unwrap(), "syncthing_user")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    assert!(
        role_default.contains("admin_user_name"),
        "the syncthing role's own default moved to `{role_default}`; every other host runs it"
    );

    let vars = agent_group_vars();
    assert!(text(&vars, "syncthing_user").contains("agent_syncthing_user_name"));

    let everywhere = repo().join("ansible/group_vars/all");
    for entry in std::fs::read_dir(everywhere).unwrap() {
        let content = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(
            !content.contains("syncthing_user:"),
            "group_vars/all binds `syncthing_user`, which reaches every host"
        );
    }

    let playbook = std::fs::read_to_string(playbooks_dir().join("memsearch.yml")).unwrap();
    assert!(
        !playbook.contains("syncthing_user"),
        "memsearch.yml binds `syncthing_user`, which reaches every host it runs on"
    );
}

#[test]
fn test_the_store_is_agent_syncthing_2770() {
    let vars = agent_group_vars();
    assert!(text(&vars, "memsearch_user").contains("agent_user_name"));
    assert!(text(&vars, "memsearch_store_group").contains("agent_syncthing_user_name"));
    assert_eq!(text(&vars, "memsearch_store_mode"), "2770");

    assert!(text(&vars, "syncthing_workspace_owner").contains("agent_user_name"));
    assert!(text(&vars, "syncthing_workspace_group").contains("agent_syncthing_user_name"));
    assert_eq!(text(&vars, "syncthing_workspace_mode"), "2770");

    let memsearch = std::fs::read_to_string(role_dir("memsearch").join("tasks/main.yml")).unwrap();
    assert_eq!(
        memsearch
            .matches("group: \"{{ memsearch_store_group }}\"")
            .count(),
        2,
        "the store root and the memory directory both take the store group"
    );
    assert_eq!(
        memsearch
            .matches("mode: \"{{ memsearch_store_mode }}\"")
            .count(),
        2,
        "the store root and the memory directory both take the store mode"
    );
}

#[test]
fn test_agent_users_is_first_on_the_agent_roster() {
    let plays = parse_yaml(&playbooks_dir().join("ruche.yml"));
    let roles = plays[0]
        .get("roles")
        .and_then(Value::as_sequence)
        .expect("ruche.yml must list roles");
    let first = roles[0]
        .get("role")
        .and_then(Value::as_str)
        .expect("a mapping roles entry declares `role`");
    assert_eq!(first, "agent_users");
}
