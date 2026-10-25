//! Every download a deploy makes is bound to a digest reviewed in this repo.
//!
//! A checksum fetched beside the asset proves nothing: whoever can swap the
//! asset can swap the sidecar too. So each `get_url` names a `<prefix>_url`
//! and checks it against a `<prefix>_sha256` (or `_checksum`) literal in the
//! same role's defaults, the pair `.github/scripts/recompute-checksums.sh`
//! maintains on a version bump. A digest computed at deploy time from a `uri:`
//! fetch resolves to no literal and fails here.
//!
//! `unarchive` and `apt deb:` take no checksum, so neither may read a URL.
//!
//! The domain is [`runnable_files`]: a download in a handler or a playbook is
//! as much a download as one in a role's `tasks/`.
//!
//! Package managers are out of its reach: `git` clones, `pip`/`uv` installs and
//! apt packages verify through their own channels, or not at all, and this
//! fence does not see them.

use serde_yaml::{Mapping, Value};

mod common;

use common::{
    Plays, defaults, field, relative, resolve, role_of, runnable_files, task_name, tasks_in,
};

/// Roles whose download the scan must keep reaching, so the fences below
/// cannot pass by looking at nothing.
const KNOWN_DOWNLOADS: &[&str] = &["forgejo", "gokapi", "navidrome", "opencode", "tgtg"];

struct ModuleCall {
    label: String,
    role: Option<String>,
    args: Mapping,
}

impl ModuleCall {
    fn raw(&self, key: &str) -> Option<&str> {
        field(&self.args, key).and_then(Value::as_str)
    }

    fn resolved(&self, key: &str) -> Option<String> {
        let vars = self.role.as_deref().map(defaults).unwrap_or_default();
        self.raw(key).map(|raw| resolve(raw, &vars))
    }
}

fn calls(module: &str) -> Vec<ModuleCall> {
    runnable_files()
        .into_iter()
        .flat_map(|path| {
            let file = relative(&path);
            let role = role_of(&file);
            tasks_in(&path, Plays::Descend)
                .into_iter()
                .filter_map(|task| {
                    let args = field(&task.body, module)?.as_mapping()?.clone();
                    Some(ModuleCall {
                        label: format!("{file}: {}", task_name(&task.body)),
                        role: role.clone(),
                        args,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn is_sha256_literal(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

fn pairing_problem(call: &ModuleCall) -> Option<String> {
    let url = call.raw("url").unwrap_or_default();
    let Some(prefix) = url
        .strip_prefix("{{ ")
        .and_then(|rest| rest.strip_suffix("_url }}"))
    else {
        return Some(format!(
            "url `{url}` is not a bare `{{{{ <prefix>_url }}}}`"
        ));
    };
    let Some(checksum) = call.raw("checksum") else {
        return Some("no `checksum:`".to_string());
    };
    let paired = [
        format!("sha256:{{{{ {prefix}_sha256 }}}}"),
        format!("{{{{ {prefix}_checksum }}}}"),
    ];
    if !paired.iter().any(|form| form == checksum) {
        return Some(format!(
            "checksum `{checksum}` is not `{prefix}_sha256` or `{prefix}_checksum`"
        ));
    }
    let resolved = call.resolved("checksum").unwrap_or_default();
    (!is_sha256_literal(&resolved)).then(|| {
        format!("checksum resolves to `{resolved}`, not a sha256 literal in the role's defaults")
    })
}

#[test]
fn the_scan_still_reaches_known_downloads() {
    let downloads = calls("ansible.builtin.get_url");
    for role in KNOWN_DOWNLOADS {
        assert!(
            downloads
                .iter()
                .any(|call| call.role.as_deref() == Some(role)),
            "{role}: no `ansible.builtin.get_url` reached; the fences below pass vacuously for it"
        );
    }
}

#[test]
fn every_download_carries_a_co_pinned_digest() {
    let offenders: Vec<String> = calls("ansible.builtin.get_url")
        .iter()
        .filter_map(|call| pairing_problem(call).map(|why| format!("{}: {why}", call.label)))
        .collect();

    assert!(
        offenders.is_empty(),
        "these downloads are not bound to a co-pinned digest; pair `<prefix>_url` with a \
         `<prefix>_sha256` literal in defaults and check against it:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn nothing_is_unpacked_or_installed_straight_from_a_url() {
    let offenders: Vec<String> = [
        ("ansible.builtin.unarchive", "src"),
        ("ansible.builtin.apt", "deb"),
    ]
    .into_iter()
    .flat_map(|(module, key)| {
        calls(module).into_iter().filter_map(move |call| {
            let source = call.resolved(key)?;
            (source.starts_with("http://") || source.starts_with("https://"))
                .then(|| format!("{}: {key} `{source}`", call.label))
        })
    })
    .collect();

    assert!(
        offenders.is_empty(),
        "these tasks read a URL with no checksum; download it with a co-pinned `get_url` \
         first:\n  {}",
        offenders.join("\n  ")
    );
}
