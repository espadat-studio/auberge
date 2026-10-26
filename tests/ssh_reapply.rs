//! The `ssh` role re-applies to a bootstrapped Host through `ssh.yml` (#1200).
//!
//! `bootstrap.yml` was the role's only playbook, and it pins `ansible_port: 22`
//! at play level, which outranks the port the runner writes into the
//! Inventory. On a hardened Host port 22 is closed, so a release that changed
//! the role could not reach a live Host. `ssh.yml` pins nothing: the
//! connection port is the Inventory's, the Host's configured one.
//!
//! The scan looks for `ansible_port` as a key (a play var, a `set_fact`, a
//! module arg), never as a value: the guard reads it, and reading is fine.
//! `bootstrap.yml` is the witness that the scan finds each key when one is set.

use serde_yaml::Value;

use auberge::services::dependency_resolver::{parse_roster, standalone_stem};

mod common;

use common::{Plays, parse_yaml, playbooks_dir, strings, tasks_in};

const CONNECTION_KEYS: [&str; 3] = ["ansible_port", "ansible_user", "remote_user"];

fn sets_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Mapping(map) => map
            .iter()
            .any(|(k, v)| k.as_str() == Some(key) || sets_key(v, key)),
        Value::Sequence(seq) => seq.iter().any(|v| sets_key(v, key)),
        Value::Tagged(tagged) => sets_key(&tagged.value, key),
        _ => false,
    }
}

#[test]
fn test_ssh_is_a_standalone_playbook_holding_the_ssh_role() {
    assert_eq!(standalone_stem("ssh.yml"), Some("ssh"));
    let roster = parse_roster(&playbooks_dir().join("ssh.yml")).unwrap();
    let names: Vec<&str> = roster.iter().map(|role| role.name.as_str()).collect();
    assert_eq!(
        names,
        ["ssh"],
        "ssh.yml must apply the ssh role and nothing else"
    );
}

#[test]
fn test_bootstrap_pins_the_connection_so_the_scan_is_not_vacuous() {
    let playbook = parse_yaml(&playbooks_dir().join("bootstrap.yml"));
    for key in CONNECTION_KEYS {
        assert!(
            sets_key(&playbook, key),
            "bootstrap.yml no longer sets `{key}`"
        );
    }
}

#[test]
fn test_ssh_connects_on_the_inventory_port_not_22() {
    let playbook = parse_yaml(&playbooks_dir().join("ssh.yml"));
    for key in CONNECTION_KEYS {
        assert!(
            !sets_key(&playbook, key),
            "ssh.yml sets `{key}`; the connection must come from the Inventory"
        );
    }
}

#[test]
fn test_ssh_refuses_to_move_the_port() {
    let guarded = tasks_in(&playbooks_dir().join("ssh.yml"), Plays::Descend)
        .iter()
        .any(|task| {
            let that = strings(
                task.body
                    .get("ansible.builtin.assert")
                    .and_then(|a| a.get("that")),
            );
            let tags = strings(task.body.get("tags"));
            that.iter()
                .any(|c| c.contains("ssh_port") && c.contains("ansible_port"))
                && tags.iter().any(|t| t == "always")
        });
    assert!(
        guarded,
        "ssh.yml must assert, tagged always, that ssh_port is the Host's port: \
         it re-applies sshd, and a port it moved would strand every later run"
    );
}
