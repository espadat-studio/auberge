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
//! `bootstrap.yml` is the witness that the scan finds a pin when one is there.

use serde_yaml::Value;

use auberge::services::dependency_resolver::{parse_roster, standalone_stem};

mod common;

use common::{parse_yaml, playbooks_dir};

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
fn test_bootstrap_pins_port_22_so_the_scan_is_not_vacuous() {
    assert!(sets_key(
        &parse_yaml(&playbooks_dir().join("bootstrap.yml")),
        "ansible_port"
    ));
}

#[test]
fn test_ssh_connects_on_the_inventory_port_not_22() {
    let playbook = parse_yaml(&playbooks_dir().join("ssh.yml"));
    for key in ["ansible_port", "ansible_user", "remote_user"] {
        assert!(
            !sets_key(&playbook, key),
            "ssh.yml sets `{key}`; the connection must come from the Inventory"
        );
    }
}

#[test]
fn test_ssh_refuses_to_move_the_port_from_under_its_connection() {
    let playbook = serde_yaml::to_string(&parse_yaml(&playbooks_dir().join("ssh.yml"))).unwrap();
    assert!(
        playbook.contains("ssh_port | int == ansible_port | int"),
        "ssh.yml must assert ssh_port matches the connection port: it does not \
         open a new port in ufw, so restarting sshd elsewhere locks the Host out"
    );
}
