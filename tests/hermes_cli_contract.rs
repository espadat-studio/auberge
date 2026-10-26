//! #1204: the hermes role calls the hermes CLI, and that CLI belongs to one
//! release.
//!
//! The role's install probe ran `hermes version` from its first version on. The
//! release `ansible/playbooks/hermes.meta.yml` pins dropped that subcommand, the
//! probe exited 2, and a deploy aborted between retiring the old gateway unit
//! and installing the new one, leaving the gateway down.
//!
//! A test cannot run the binary on the Host, so it cannot check the call. What
//! it can do is hold the subcommand list a human read out of the pinned release,
//! fail every role call outside it, and fail when the pin moves without that
//! list being read again.

use auberge::playbook_meta::declared_app_versions;

mod common;

use common::{playbooks_dir, relative, role_template_files, role_yml_files};

const APP: &str = "hermes";

/// The release [`SUBCOMMANDS`] was read from.
const VERIFIED_RELEASE: &str = "v2026.9.24";

/// The top-level subcommands of hermes 0.21.5, read from the
/// `subparsers.add_parser(...)` calls under `hermes_cli/` at [`VERIFIED_RELEASE`].
/// `version` is not among them; `--version` is a top-level flag.
const SUBCOMMANDS: &[&str] = &[
    "acp",
    "approvals",
    "auth",
    "backup",
    "browser",
    "bundles",
    "chat",
    "checkpoints",
    "claw",
    "codex-runtime",
    "completion",
    "computer-use",
    "config",
    "console",
    "cron",
    "curator",
    "dashboard",
    "debug",
    "desktop",
    "doctor",
    "dump",
    "egress",
    "fallback",
    "gateway",
    "hooks",
    "import",
    "import-agent",
    "insights",
    "journey",
    "login",
    "logout",
    "logs",
    "mcp",
    "memory",
    "migrate",
    "moa",
    "model",
    "monitoring",
    "pairing",
    "pause",
    "peer",
    "pets",
    "plugins",
    "portal",
    "profile",
    "prompt-size",
    "proxy",
    "resume",
    "secrets",
    "security",
    "send",
    "serve",
    "sessions",
    "setup",
    "skills",
    "skin",
    "slack",
    "status",
    "sync",
    "tools",
    "uninstall",
    "update",
    "usage",
    "vault",
    "verify",
    "webhook",
    "whatsapp",
    "whatsapp-cloud",
    "worktree",
];

/// The word after each `bin/hermes ` in the role's tasks, handlers and
/// templates, with the file it sits in. A path without a following space is the
/// binary named as a file, not called.
fn role_calls() -> Vec<(String, String)> {
    let mut files = role_yml_files(APP);
    files.extend(role_template_files(APP));
    let mut calls = Vec::new();
    for file in files {
        let raw = std::fs::read_to_string(&file).unwrap();
        for (at, _) in raw.match_indices("bin/hermes") {
            let rest = &raw[at + "bin/hermes".len()..];
            if !rest.starts_with([' ', '\t']) {
                continue;
            }
            let word: String = rest
                .trim_start_matches([' ', '\t'])
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
                .collect();
            calls.push((relative(&file), word));
        }
    }
    calls
}

#[test]
fn every_role_call_names_a_subcommand_the_pinned_release_lists() {
    for (file, word) in role_calls() {
        if word.starts_with('-') {
            continue;
        }
        assert!(
            SUBCOMMANDS.contains(&word.as_str()),
            "{file} calls `hermes {word}`, which hermes {VERIFIED_RELEASE} does not list"
        );
    }
}

/// Without this the check above passes over a role it never reached.
#[test]
fn the_walk_reaches_the_gateway_unit_and_the_install_probe() {
    let words: Vec<String> = role_calls().into_iter().map(|(_, word)| word).collect();
    for expected in ["gateway", "--version"] {
        assert!(
            words.iter().any(|w| w == expected),
            "expected the hermes role to call `hermes {expected}`, found {words:?}"
        );
    }
}

#[test]
fn the_verified_release_is_the_one_the_playbook_pins() {
    let versions = declared_app_versions(&playbooks_dir()).expect("playbook metas must parse");
    let (_, pin) = versions
        .iter()
        .find(|(app, _)| app == APP)
        .unwrap_or_else(|| panic!("{APP}.meta.yml must declare an App Version (ADR-0017)"));
    assert_eq!(pin.dep_name, "NousResearch/hermes-agent");
    assert_eq!(
        VERIFIED_RELEASE, pin.value,
        "\nSUBCOMMANDS was read from hermes {VERIFIED_RELEASE}, but \
         ansible/playbooks/{APP}.meta.yml now deploys {}.\n\
         Re-read the `subparsers.add_parser(...)` calls under `hermes_cli/` at the new \
         tag, update SUBCOMMANDS, then move VERIFIED_RELEASE.",
        pin.value
    );
}
