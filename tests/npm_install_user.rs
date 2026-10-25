//! npm runs dependency lifecycle scripts during an install, and the tree they
//! come from is resolved at install time, not pinned by the top-level version.
//! An npm task therefore runs as an unprivileged user, never under the play's
//! root `become`.

mod common;

use serde_yaml::Value;

use common::{Plays, field, relative, runnable_files, task_name, tasks_in};

#[test]
fn test_every_npm_install_runs_as_an_unprivileged_user() {
    let mut seen = Vec::new();
    let mut offenders = Vec::new();
    for path in runnable_files() {
        for task in tasks_in(&path, Plays::Descend) {
            if field(&task.body, "community.general.npm").is_none() {
                continue;
            }
            seen.push(relative(&path));
            let user = field(&task.body, "become_user").and_then(Value::as_str);
            if matches!(user, None | Some("root")) {
                offenders.push(format!("{}: {}", relative(&path), task_name(&task.body)));
            }
        }
    }
    assert!(
        seen.iter()
            .any(|path| path == "ansible/roles/actual/tasks/main.yml"),
        "the scan must reach the actual role's npm install: {seen:?}"
    );
    assert!(
        offenders.is_empty(),
        "npm installs must set a non-root become_user: {offenders:?}"
    );
}
