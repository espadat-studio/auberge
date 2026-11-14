//! An independent witness for the Read Set (#1322).
//!
//! A deploy resolves a `!` value only when its key is in the run's Read Set,
//! which `auberge::services::read_set` computes by parsing expressions. A
//! name that walk fails to see is a secret the run reads and never gets: the
//! role falls back to its default, or dies on an undefined variable.
//!
//! So this states the expectation without the walk. Every Key Registry name
//! that appears as a whole word anywhere in a file the run renders must be in
//! the run's Read Set. Which roles a run reaches is read off the raw text
//! with regexes, never through the crate. Word matching over-approximates on
//! purpose: a name in a comment or a literal costs a line here, never a
//! missing secret on a host.

mod common;

use auberge::services::read_set::ReadSet;
use auberge::services::required_keys::read_set_for;
use common::{
    all_roles, playbook_files, registry_keys, relative, repo, role_dir, role_template_files,
    role_yml_files, yml_files,
};
use regex::Regex;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static ROLE_ENTRY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)\brole:\s*['"]?([\w.-]+)"#).unwrap());
static BARE_DEPENDENCY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*-\s*['"]?([\w.-]+)['"]?\s*$"#).unwrap());
static INCLUDED_ROLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:include_role|import_role):\s*\n\s+name:\s*['"]?([\w.-]+)"#).unwrap()
});

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", relative(path)))
}

fn role_text_files(role: &str) -> Vec<PathBuf> {
    let mut files = role_yml_files(role);
    files.extend(role_template_files(role));
    files
}

/// Roles named in `body`, by any edge Ansible follows, limited to roles that
/// exist so a list item in `galaxy_info` is not mistaken for one.
fn named_roles(body: &str, known: &BTreeSet<String>) -> BTreeSet<String> {
    ROLE_ENTRY
        .captures_iter(body)
        .chain(INCLUDED_ROLE.captures_iter(body))
        .chain(BARE_DEPENDENCY.captures_iter(body))
        .map(|caps| caps[1].to_string())
        .filter(|name| known.contains(name))
        .collect()
}

fn reached_files(playbook: &Path) -> Vec<PathBuf> {
    let known: BTreeSet<String> = all_roles().into_iter().collect();
    let mut roles = named_roles(&text(playbook), &known);
    let mut pending: Vec<String> = roles.iter().cloned().collect();
    while let Some(role) = pending.pop() {
        for path in role_text_files(&role) {
            for found in named_roles(&text(&path), &known) {
                if roles.insert(found.clone()) {
                    pending.push(found);
                }
            }
        }
    }
    let mut files = vec![playbook.to_path_buf()];
    for role in &roles {
        assert!(role_dir(role).is_dir(), "{role} has no directory");
        files.extend(role_text_files(role));
    }
    files.extend(yml_files(&repo().join("ansible/group_vars")));
    files.push(repo().join("ansible/inventory.yml"));
    files
}

#[test]
fn test_every_registry_name_a_run_mentions_is_in_its_read_set() {
    let ansible = repo().join("ansible");
    let registry = registry_keys();
    let mut gaps = Vec::new();
    let mut narrowed = Vec::new();

    for playbook in playbook_files() {
        let name = playbook.file_name().unwrap().to_string_lossy().to_string();
        let read_set =
            read_set_for(&ansible, &name, None).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        if read_set == ReadSet::Everything {
            continue;
        }
        narrowed.push(name.clone());
        let corpus: String = reached_files(&playbook)
            .iter()
            .map(|path| text(path))
            .collect::<Vec<_>>()
            .join("\n");
        for key in &registry {
            let word = Regex::new(&format!(r"\b{}\b", regex::escape(key))).unwrap();
            if word.is_match(&corpus) && !read_set.contains(key) {
                gaps.push(format!("  {name}: {key}"));
            }
        }
    }

    assert!(
        narrowed.len() >= 14,
        "only {} run(s) narrow their Read Set, was 15 of 16 ({narrowed:?}); the \
         witness checks nothing for a run whose Read Set is everything",
        narrowed.len()
    );
    assert!(
        gaps.is_empty(),
        "registry key(s) a run's files mention but its Read Set leaves out. A `!` \
         value on one is never resolved for that run, so the role reads its \
         default or dies on an undefined variable. Teach \
         `auberge::services::read_set` the reference it misses:\n{}",
        gaps.join("\n")
    );
}
