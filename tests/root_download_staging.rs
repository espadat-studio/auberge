//! Root stages what it downloads in a directory only root can write.
//!
//! A fixed name in the shared `/tmp` can be pre-created by any local user, and
//! `get_url` then either keeps the planted bytes or hands the fresh ones to the
//! planter's ownership before a later root task consumes them by name. Every
//! download, unpack, `.deb` install and remote copy therefore stages under
//! `root_download_dir`, which each role that uses it creates root-owned 0700.

use std::collections::BTreeMap;

use serde_yaml::{Mapping, Value};

mod common;

use common::{
    all_roles, defaults, field, group_vars_all, relative, resolve, role_dir, role_tasks, task_name,
};

const SHARED_TMP: &[&str] = &["/tmp/", "/var/tmp/"];

const STAGING_ROLES: &[&str] = &[
    "aoe",
    "baikal",
    "bichon",
    "blocky",
    "colporteur",
    "gokapi",
    "grimmory",
    "headscale",
    "navidrome",
    "opencode",
    "paperless",
    "stirling",
];

fn vars(role: &str) -> BTreeMap<String, String> {
    let mut vars = group_vars_all();
    vars.extend(defaults(role));
    vars
}

fn module<'a>(task: &'a Mapping, name: &str) -> Option<&'a Mapping> {
    field(task, &format!("ansible.builtin.{name}")).and_then(Value::as_mapping)
}

fn text(args: &Mapping, key: &str) -> Option<String> {
    field(args, key).and_then(Value::as_str).map(str::to_string)
}

fn is_true(args: &Mapping, key: &str) -> Option<bool> {
    field(args, key).and_then(Value::as_bool)
}

/// The on-host paths a task writes a download to or reads one back from.
fn staged_paths(task: &Mapping) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(args) = module(task, "get_url") {
        paths.extend(text(args, "dest"));
    }
    if let Some(args) = module(task, "unarchive") {
        let remote =
            is_true(args, "remote_src") == Some(true) || is_true(args, "copy") == Some(false);
        if remote {
            paths.extend(text(args, "src"));
        }
    }
    if let Some(args) = module(task, "apt") {
        paths.extend(text(args, "deb"));
    }
    if let Some(args) = module(task, "copy")
        && is_true(args, "remote_src") == Some(true)
    {
        paths.extend(text(args, "src"));
    }
    paths
}

fn creates_private_dir(task: &Mapping, dir: &str, vars: &BTreeMap<String, String>) -> bool {
    let Some(args) = module(task, "file") else {
        return false;
    };
    let resolved = |key| text(args, key).map(|raw| resolve(&raw, vars));
    resolved("path").as_deref() == Some(dir)
        && resolved("state").as_deref() == Some("directory")
        && resolved("owner").as_deref() == Some("root")
        && resolved("group").as_deref() == Some("root")
        && resolved("mode").as_deref() == Some("0700")
}

#[test]
fn no_root_download_is_staged_in_shared_tmp() {
    let mut seen = 0;
    let mut offenders = Vec::new();
    for role in all_roles() {
        let vars = vars(&role);
        for task in role_tasks(&role) {
            for raw in staged_paths(&task.body) {
                seen += 1;
                let path = resolve(&raw, &vars);
                if SHARED_TMP.iter().any(|tmp| path.starts_with(tmp)) {
                    offenders.push(format!(
                        "{role}: `{}` stages at {path}",
                        task_name(&task.body)
                    ));
                }
            }
        }
    }
    assert!(
        seen >= 20,
        "only {seen} staged paths found; the walk lost its domain"
    );
    assert!(
        offenders.is_empty(),
        "staged in shared tmp:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn a_role_staging_in_the_download_dir_creates_it_root_private() {
    let mut staging_roles = Vec::new();
    let mut offenders = Vec::new();
    for role in all_roles() {
        let vars = vars(&role);
        let dir = resolve("{{ root_download_dir }}", &vars);
        assert!(
            !dir.contains("{{"),
            "root_download_dir must resolve from group_vars/all"
        );
        let tasks = role_tasks(&role);
        let stages = tasks
            .iter()
            .flat_map(|task| staged_paths(&task.body))
            .any(|raw| resolve(&raw, &vars).starts_with(&format!("{dir}/")));
        if !stages {
            continue;
        }
        staging_roles.push(role.clone());
        if !tasks
            .iter()
            .any(|task| creates_private_dir(&task.body, &dir, &vars))
        {
            offenders.push(relative(&role_dir(&role)));
        }
    }
    assert_eq!(
        staging_roles, STAGING_ROLES,
        "the roles staging in root_download_dir moved; update STAGING_ROLES"
    );
    assert!(
        offenders.is_empty(),
        "stages without creating root_download_dir root 0700:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn xcaddy_repository_is_declared_without_a_download() {
    let tasks = role_tasks("caddy");
    let fetched = tasks
        .iter()
        .filter_map(|task| module(&task.body, "get_url"))
        .filter_map(|args| text(args, "url"));
    assert!(
        fetched
            .into_iter()
            .all(|url| !url.ends_with("debian.deb.txt")),
        "the xcaddy source list must be declared in the role, not fetched"
    );
    let declared = tasks
        .iter()
        .filter_map(|task| module(&task.body, "copy"))
        .any(|args| {
            text(args, "dest").as_deref() == Some("/etc/apt/sources.list.d/caddy-xcaddy.sources")
                && text(args, "content").is_some_and(|content| {
                    content
                        .contains("Signed-By: /usr/share/keyrings/caddy-xcaddy-archive-keyring.gpg")
                })
        });
    assert!(
        declared,
        "caddy must declare a deb822 xcaddy source with Signed-By"
    );
}
