//! hermes runs as its own unprivileged user (ADR-0087).
//!
//! The gateway takes instructions from Telegram and has a local terminal
//! tool, so it must not share an identity with a sudo-capable user. Two
//! things keep that from coming back:
//!
//! - the role's runtime user is `hermes`, never `admin_user_name`;
//! - no hermes task grants a privileged group or touches sudoers.

mod common;

use common::{defaults, role_tasks};

const PRIVILEGED_GROUPS: &[&str] = &["sudo", "docker", "adm", "wheel", "root", "sshusers"];

#[test]
fn test_hermes_runs_as_its_own_user() {
    let vars = defaults("hermes");
    for key in ["hermes_user", "hermes_group"] {
        let value = vars
            .get(key)
            .unwrap_or_else(|| panic!("hermes defaults must declare `{key}`"));
        assert_eq!(
            value, "hermes",
            "`{key}` must not fall back to the admin user"
        );
    }
}

#[test]
fn test_hermes_creates_its_user_without_privilege() {
    let tasks = role_tasks("hermes");
    assert!(
        tasks
            .iter()
            .any(|t| t.body.get("ansible.builtin.user").is_some()),
        "the hermes role must create the user it runs as"
    );
    for task in tasks {
        let body = serde_yaml::to_string(&task.body).unwrap();
        assert!(
            !body.contains("sudoers"),
            "hermes task `{}` touches sudoers",
            common::task_name(&task.body)
        );
        for group in PRIVILEGED_GROUPS {
            assert!(
                !body.contains(&format!("[{group}]")) && !body.contains(&format!("- {group}\n")),
                "hermes task `{}` names the `{group}` group",
                common::task_name(&task.body)
            );
        }
    }
}
