//! A file that carries a secret is deployed `0600`.
//!
//! A secret rendered into a world-readable file is readable by every account on
//! the Host, service users included — and a systemd unit under
//! `/etc/systemd/system` is world-readable by design, so a unit can never
//! carry one. lego's renewal unit inlined the Zone DNS token that way; it now
//! reads a root-only `EnvironmentFile` instead.
//!
//! The domain is every `template` and `copy` task in every role. A task is in
//! scope when what it renders names a secret inside `{{ }}` or `{% %}`; it
//! passes when its `mode:` grants nothing to group or other (`0600`, `0400`).
//! [`GROUP_READERS`] names the files a service reads through its group, and
//! those still grant nothing to other. A secret is a Key Registry key
//! marked `secret: true`, a per-App `<app>_dns_api_token` the CLI injects from
//! its Zone, or a role default that renders either.

use std::collections::BTreeSet;
use std::fs;

use auberge::key_registry::KeyRegistry;
use auberge::services::zone::DNS_TOKEN_SUFFIX;
use serde_yaml::{Mapping, Value};

mod common;

use common::{all_roles, defaults, field, relative, repo, role_dir, role_tasks, task_name};

/// Computed vars that carry a secret under a name nothing else marks: each
/// Zone's token rides inside `host_zones` (CONTEXT.md, Computed Var).
const COMPUTED_SECRETS: &[&str] = &["host_zones"];

/// `(role, task)` whose secret a service reads as the file's group.
const GROUP_READERS: &[(&str, &str)] = &[
    // The opencode process reads its provider key as group `opencode`; root
    // owns the file so the agent cannot rewrite it (ADR-0064).
    ("opencode", "Deploy the OpenRouter API key"),
];

fn mode_bits(mode: &str) -> Option<u32> {
    u32::from_str_radix(mode, 8).ok()
}

fn is_private(role: &str, task: &str, mode: &str) -> bool {
    let Some(bits) = mode_bits(mode) else {
        return false;
    };
    let denied = if GROUP_READERS.contains(&(role, task)) {
        0o007
    } else {
        0o077
    };
    bits & denied == 0
}

const RENDER_MODULES: &[&str] = &[
    "ansible.builtin.template",
    "template",
    "ansible.builtin.copy",
    "copy",
];

fn registry_secrets() -> BTreeSet<String> {
    let registry =
        KeyRegistry::load(&repo().join("ansible/keys.yml")).expect("ansible/keys.yml must load");
    registry
        .iter()
        .filter(|(_, entry)| entry.secret)
        .map(|(name, _)| name.clone())
        .chain(COMPUTED_SECRETS.iter().map(|name| name.to_string()))
        .collect()
}

/// The text inside every `{{ }}` and `{% %}`, which is all a template evaluates.
fn expressions(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for (open, close) in [("{{", "}}"), ("{%", "%}")] {
        let mut rest = text;
        while let Some(start) = rest.find(open) {
            let after = &rest[start + 2..];
            let Some(end) = after.find(close) else { break };
            found.push(&after[..end]);
            rest = &after[end + 2..];
        }
    }
    found
}

fn expression_idents(text: &str) -> impl Iterator<Item = &str> {
    expressions(text)
        .into_iter()
        .flat_map(|expr| expr.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')))
}

fn is_secret(name: &str, secrets: &BTreeSet<String>) -> bool {
    secrets.contains(name) || name.ends_with(DNS_TOKEN_SUFFIX)
}

fn renders_secret(text: &str, secrets: &BTreeSet<String>) -> Option<String> {
    expression_idents(text)
        .find(|name| is_secret(name, secrets))
        .map(str::to_string)
}

/// The registry's secrets, plus every default of this role that renders one.
fn role_secrets(role: &str, registry: &BTreeSet<String>) -> BTreeSet<String> {
    let vars = defaults(role);
    let mut secrets = registry.clone();
    loop {
        let before = secrets.len();
        for (name, value) in &vars {
            if renders_secret(value, &secrets).is_some() {
                secrets.insert(name.clone());
            }
        }
        if secrets.len() == before {
            return secrets;
        }
    }
}

fn module_args(task: &Mapping) -> Option<&Mapping> {
    RENDER_MODULES
        .iter()
        .find_map(|module| field(task, module).and_then(Value::as_mapping))
}

fn rendered_text(role: &str, args: &Mapping) -> Option<String> {
    if let Some(content) = field(args, "content").and_then(Value::as_str) {
        return Some(content.to_string());
    }
    let src = field(args, "src").and_then(Value::as_str)?;
    if field(args, "remote_src").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let dir = role_dir(role);
    ["templates", "files"]
        .iter()
        .map(|sub| dir.join(sub).join(src))
        .find(|path| path.is_file())
        .map(|path| {
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", relative(&path)))
        })
}

struct Violation {
    role: String,
    task: String,
    secret: String,
    mode: String,
}

fn violations() -> (usize, Vec<Violation>) {
    let registry = registry_secrets();
    assert!(!registry.is_empty(), "the Key Registry marks no key secret");

    let mut in_scope = 0;
    let mut found = Vec::new();
    for role in all_roles() {
        let secrets = role_secrets(&role, &registry);
        for task in role_tasks(&role) {
            let Some(args) = module_args(&task.body) else {
                continue;
            };
            let Some(text) = rendered_text(&role, args) else {
                continue;
            };
            let Some(secret) = renders_secret(&text, &secrets) else {
                continue;
            };
            in_scope += 1;
            let mode = field(args, "mode")
                .and_then(Value::as_str)
                .unwrap_or("<unset>")
                .to_string();
            let name = task_name(&task.body);
            if !is_private(&role, name, &mode) {
                found.push(Violation {
                    role: role.clone(),
                    task: name.to_string(),
                    secret,
                    mode,
                });
            }
        }
    }
    (in_scope, found)
}

#[test]
fn every_file_that_renders_a_secret_is_owner_private() {
    let (_, found) = violations();
    let report: Vec<String> = found
        .iter()
        .map(|v| {
            format!(
                "{}: '{}' renders {} at mode {}",
                v.role, v.task, v.secret, v.mode
            )
        })
        .collect();
    assert!(
        report.is_empty(),
        "files carrying a secret must grant nothing to group or other:\n{}",
        report.join("\n")
    );
}

#[test]
fn the_scan_reaches_the_lego_token_file() {
    let (in_scope, _) = violations();
    assert!(in_scope > 0, "no template or copy task renders a secret");
    let registry = registry_secrets();
    let secrets = role_secrets("blocky", &registry);
    let reached = role_tasks("blocky").iter().any(|task| {
        module_args(&task.body)
            .and_then(|args| rendered_text("blocky", args))
            .and_then(|text| renders_secret(&text, &secrets))
            .is_some_and(|secret| secret == "blocky_dns_api_token")
    });
    assert!(
        reached,
        "the scan no longer sees blocky's DNS token in any deployed file"
    );
}

#[test]
fn only_owner_bits_pass_unless_the_group_is_a_named_reader() {
    assert!(is_private("bichon", "any", "0600"));
    assert!(is_private("bichon", "any", "0400"));
    assert!(!is_private("blocky", "any", "0640"));
    assert!(!is_private("blocky", "any", "0644"));
    assert!(!is_private("blocky", "any", "<unset>"));
    assert!(is_private(
        "opencode",
        "Deploy the OpenRouter API key",
        "0640"
    ));
    assert!(!is_private(
        "opencode",
        "Deploy the OpenRouter API key",
        "0644"
    ));
}

#[test]
fn a_secret_inside_an_expression_is_seen_and_plain_text_is_not() {
    let secrets = BTreeSet::from(["admin_user_password".to_string()]);
    assert_eq!(
        renders_secret("PASS='{{ admin_user_password }}'", &secrets).as_deref(),
        Some("admin_user_password")
    );
    assert_eq!(
        renders_secret(
            "{% if x %}{{ navidrome_dns_api_token | trim }}{% endif %}",
            &secrets
        )
        .as_deref(),
        Some("navidrome_dns_api_token")
    );
    assert_eq!(renders_secret("# admin_user_password", &secrets), None);
    assert_eq!(
        renders_secret("{{ navidrome_dns_api_token_env }}", &secrets),
        None
    );
}
