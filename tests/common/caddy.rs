//! The caddy layer over the walk: which roles can restart it, and which
//! vhosts they deploy.
//!
//! Three fences read one half or the other, which is why both live here
//! rather than in the first file that needed them (#654's lesson, one layer
//! up). `ingress_gate` asks which roles can restart caddy, because a restart
//! is what makes a bad vhost fatal. `vhost_acme_token` asks which vhosts
//! exist, because every one of them has to answer its own DNS-01 challenge.
//! `vhost_file_identity` asks *both*: the vhost set is its domain, and the
//! restart set is what tells it that domain is complete — a role that
//! restarts caddy and deploys no vhost this walk can see is a vhost the fence
//! is not reading (#954).
//!
//! Before that third fence the two halves sat in two files and never met,
//! which is the shape where a walk narrowed to nothing keeps passing.
//!
//! A fourth file spells [`RESTART_HANDLER`] without reading it:
//! `probe_after_restart.rs`'s `DEFERRED_HANDLERS` names the string to say this
//! handler's flush stays at end of play, which is a claim about the handler
//! rather than a question about the tree. Named here so its absence does not
//! read as coverage.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde_yaml::Value;

use super::{all_roles, field, relative, role_dir, role_tasks, yml_files};

/// The handler every vhost writer notifies.
pub const RESTART_HANDLER: &str = "Restart caddy";

/// The directory caddy imports its sites from, and so what makes a template a
/// vhost: they are found by where they land, not by what they are called. Half
/// the tree spells one `Caddyfile.j2` and half `<app>.caddyfile.j2`.
pub const SITES_DIR: &str = "/etc/caddy/sites/";

/// One site the tree deploys.
pub struct Vhost {
    pub role: String,
    /// The template's file name, without the `templates/` prefix the task
    /// spells.
    pub template: String,
    /// The path on the Host, exactly as the task writes it — interpolation
    /// included, which is what `vhost_file_identity` reads it for.
    pub dest: String,
}

impl Vhost {
    /// `role/template`, for a failure message a reader can open.
    pub fn id(&self) -> String {
        format!("{}/{}", self.role, self.template)
    }

    /// The template's own bytes, read on demand — the walk itself does not
    /// touch them, so a fence reading only `dest` never opens a file.
    ///
    /// A file that cannot be read is a hard stop, for the caller that asks:
    /// the task names it, so a missing one means the tree moved under the
    /// fence.
    pub fn body(&self) -> String {
        let path: PathBuf = role_dir(&self.role).join("templates").join(&self.template);
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", relative(&path)))
    }
}

/// Every vhost in the tree, found through the task that deploys it — so a role
/// that stops writing a site drops out and a new one is picked up, with no
/// list to update.
pub fn vhosts() -> Vec<Vhost> {
    let mut found = Vec::new();
    for role in all_roles() {
        for task in role_tasks(&role) {
            let Some(args) =
                field(&task.body, "ansible.builtin.template").and_then(Value::as_mapping)
            else {
                continue;
            };
            // `dest` and nothing else: the template action plugin reads
            // `args['dest']` with no alias, so a task spelling it any other
            // way fails the deploy with "src and dest are required" rather
            // than slipping past this walk.
            let dest = field(args, "dest").and_then(Value::as_str).unwrap_or("");
            if !dest.contains(SITES_DIR) {
                continue;
            }
            let src = field(args, "src")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{role}: a vhost task must name a src"));
            found.push(Vhost {
                role: role.clone(),
                template: src.trim_start_matches("templates/").to_string(),
                dest: dest.to_string(),
            });
        }
    }
    found
}

/// Roles with a task that notifies [`RESTART_HANDLER`]. A restart is what makes
/// a bad vhost fatal: `caddy reload` validates and keeps the running config,
/// but a restart replaces it, so a vhost binding an address the host does not
/// own takes every other vhost down with it.
///
/// Read as text rather than as parsed `notify:` lists on purpose. This is the
/// discovery half of the ingress fence, and it is allowed to over-report: a
/// role that merely mentions the handler is gated too, which costs a
/// `post_task` and nothing else. Under-reporting is what takes the fleet down,
/// so the loose read is the safe direction — and both fences over it assert it
/// found somebody, so it cannot silently report nobody.
pub fn roles_that_restart_caddy() -> BTreeSet<String> {
    all_roles()
        .into_iter()
        .filter(|role| {
            yml_files(&role_dir(role).join("tasks"))
                .iter()
                .any(|file| fs::read_to_string(file).unwrap().contains(RESTART_HANDLER))
        })
        .collect()
}
