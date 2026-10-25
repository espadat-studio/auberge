//! A recursive `file` task runs as root over a tree its App's service user can
//! write, so everything the walk touches has to be inside that tree.
//!
//! The module defaults `follow` to true: a symlink the service user plants in
//! its own tree hands the task's owner and mode to whatever the link points at,
//! on the next deploy. `follow: false` makes the walk act on the link itself.
//!
//! A recursive octal mode is the other half. It gives every file the
//! directory's bits, so a database inside the tree ends up as readable as the
//! directory holding it. A symbolic mode with `X` keeps the two apart, but `X`
//! also keeps an `x` a file already has, so a database tree's mode opens with
//! `a-x` to clear what the old `0755` left. Clauses apply in order, as `chmod`.
//!
//! The domain is [`runnable_files`], the widest one: a recursive chown is as
//! dangerous in a handler or a playbook's own tasks as in a role's `tasks/`.
//!
//! A recursive task over a database tree's ancestor reaches the database too,
//! and a later task narrowing the tree again only closes the window it opened.
//! Baikal's walk over all of `/opt/baikal` left the database, `config/` and the
//! busy sync secrets readable by other between two tasks of every deploy.

use serde_yaml::{Mapping, Value};

mod common;

use common::{
    Plays, defaults, field, relative, resolve, runnable_files, strings, task_name, tasks_in,
};

/// Trees that hold an App's database, by role and resolved path.
const DATABASE_TREES: &[(&str, &str)] = &[
    ("navidrome", "/var/lib/navidrome"),
    ("calibre", "/srv/calibre"),
    ("baikal", "/opt/baikal/Specific"),
];

struct RecursiveFileTask {
    label: String,
    role: Option<String>,
    args: Mapping,
    items: Vec<String>,
}

impl RecursiveFileTask {
    fn arg(&self, key: &str) -> Option<&str> {
        field(&self.args, key).and_then(Value::as_str)
    }

    fn paths(&self, role: &str) -> Vec<String> {
        let Some(path) = self.arg("path") else {
            return Vec::new();
        };
        let mut vars = defaults(role);
        if self.items.is_empty() {
            return vec![resolve(path, &vars)];
        }
        self.items
            .iter()
            .map(|item| {
                vars.insert("item".to_string(), item.clone());
                resolve(path, &vars)
            })
            .collect()
    }

    fn is_over(&self, role: &str, tree: &str) -> bool {
        self.role.as_deref() == Some(role) && self.paths(role).iter().any(|path| path == tree)
    }

    fn covers(&self, role: &str, tree: &str) -> bool {
        self.role.as_deref() == Some(role)
            && self.paths(role).iter().any(|path| {
                tree == path
                    || tree
                        .strip_prefix(path.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
    }
}

fn recursive_file_tasks() -> Vec<RecursiveFileTask> {
    runnable_files()
        .into_iter()
        .flat_map(|path| {
            let file = relative(&path);
            let role = file
                .strip_prefix("ansible/roles/")
                .and_then(|rest| rest.split('/').next())
                .map(str::to_string);
            tasks_in(&path, Plays::Descend)
                .into_iter()
                .filter_map(move |task| {
                    let args = field(&task.body, "ansible.builtin.file")?
                        .as_mapping()?
                        .clone();
                    (field(&args, "recurse") == Some(&Value::Bool(true))).then(|| {
                        RecursiveFileTask {
                            label: format!("{file}: {}", task_name(&task.body)),
                            role: role.clone(),
                            args,
                            items: strings(field(&task.body, "loop")),
                        }
                    })
                })
        })
        .collect()
}

#[test]
fn the_scan_still_reaches_every_database_tree() {
    let tasks = recursive_file_tasks();
    for (role, tree) in DATABASE_TREES {
        assert!(
            tasks.iter().any(|task| task.is_over(role, tree)),
            "{role}: no recursive `ansible.builtin.file` task over `{tree}` is reached; \
             the fences below now pass vacuously for it"
        );
    }
}

#[test]
fn every_recursive_file_task_never_follows_links() {
    let offenders: Vec<String> = recursive_file_tasks()
        .into_iter()
        .filter(|task| field(&task.args, "follow") != Some(&Value::Bool(false)))
        .map(|task| task.label)
        .collect();

    assert!(
        offenders.is_empty(),
        "these recursive file tasks follow symlinks as root; set `follow: false`:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn every_recursive_mode_keeps_files_apart_from_directories() {
    let offenders: Vec<String> = recursive_file_tasks()
        .iter()
        .filter_map(|task| {
            let mode = task.arg("mode")?;
            mode.chars()
                .all(|c| c.is_ascii_digit())
                .then(|| format!("{} ({mode})", task.label))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these recursive file tasks give files the directory's octal mode; \
         use a symbolic mode with `X`:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn database_trees_are_private_and_execute_nothing() {
    let tasks = recursive_file_tasks();
    for (role, tree) in DATABASE_TREES {
        for task in tasks.iter().filter(|task| task.covers(role, tree)) {
            let mode = task
                .arg("mode")
                .unwrap_or_else(|| panic!("{}: no mode", task.label));
            assert!(
                mode.starts_with("a-x,"),
                "{}: `{mode}` keeps the execute bit an old octal mode left on the files in \
                 `{tree}`; open it with `a-x,` so `X` only marks directories",
                task.label
            );
            let other = mode
                .split(',')
                .find_map(|clause| clause.strip_prefix("o="))
                .unwrap_or_else(|| panic!("{}: `{mode}` does not set `o=`", task.label));
            assert!(
                other.is_empty(),
                "{}: `{mode}` lets other read the database in `{tree}`",
                task.label
            );
        }
    }
}
