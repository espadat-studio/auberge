//! A vhost file is named after its **App**, never after the App's FQDN.
//!
//! Every role wrote its site to `/etc/caddy/sites/{{ <app>_domain }}.caddyfile`
//! — a filename that moves whenever the name it serves moves — and nothing in
//! the tree removes a vhost file. So changing an App's subdomain, or its
//! **Zone**, wrote a second vhost beside the first. Caddy imports
//! `/etc/caddy/sites/*.caddyfile`, so it went on serving both names and
//! renewing both certificates; `dns_record` creates and updates only, so the
//! old A record survived too. A move silently became a duplicate (#954).
//!
//! The filename is the App's **identity**; the name it serves is an
//! *attribute*, and it stays where it always was — inside the file, on the
//! site line. Redeploying after a Zone change now overwrites one file rather
//! than creating a second.
//!
//! ## What the rename made possible
//!
//! An FQDN-derived path was unique by construction: two roles could not
//! collide, because two Apps cannot hold one public name. A hand-written
//! literal can. `test_no_two_vhosts_write_the_same_file` is the assertion
//! that did not need to exist before this change and does now.
//!
//! ## Reach
//!
//! Stated twice over, and both drift. As counts — 18 sites across 17 roles,
//! the pair `vhost_acme_token.rs` states for its own assertions, so a new
//! vhost moves both files. And against the **other** half of
//! `common::caddy`: every role that can restart caddy, save caddy itself,
//! must deploy a site this walk found. A count alone cannot say that. A walk
//! narrowed to nothing satisfies every `for` loop below, and the restart set
//! is discovered by unrelated means — reading task text for the handler name
//! — so narrowing both at once takes two edits, in two directions.
//!
//! ## Not fenced here
//!
//! Removing what the old naming already left on each Host. A role cannot know
//! which stale names were once its own, so the sweep is a one-time operator
//! step, written down with the Zone cutover it lands beside. It is
//! `find /etc/caddy/sites -name '*.*.caddyfile' -delete`, and
//! `test_no_vhost_file_matches_the_operators_sweep_pattern` is what keeps
//! that pattern unable to match a live site.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde_yaml::Value;

mod common;

use common::apps::app_of;
use common::caddy::{SITES_DIR, Vhost, roles_that_restart_caddy, vhosts};
use common::{field, relative, role_dir, role_tasks};

/// The extension caddy's own `import` globs for. A vhost written under any
/// other name is deployed, owned, and never read.
const EXTENSION: &str = ".caddyfile";

/// Caddy's own config, where the glob every assertion below argues from lives.
const MAIN_CONFIG: &str = "/etc/caddy/Caddyfile";

/// The one role that restarts caddy without serving a site of its own: it
/// owns the process and the `import` line, and the Ingress Gate is its probe.
const OWNS_THE_PROCESS: &str = "caddy";

/// `dest` reduced to the name the two assertions below read, with the two
/// premises that make a name the whole story checked on the way: the file lands
/// directly in the imported directory, and it carries the imported extension.
///
/// Both are hard stops rather than skips. A site failing either is deployed,
/// owned, `0644` — and never read, because caddy's `import` is one
/// non-recursive glob.
fn stem(vhost: &Vhost) -> String {
    let (id, dest) = (vhost.id(), &vhost.dest);
    let under = dest
        .strip_prefix(SITES_DIR)
        .unwrap_or_else(|| panic!("{id} writes `{dest}`, which is not under {SITES_DIR}"));
    assert!(
        !under.contains('/'),
        "{id} writes `{dest}`, a path below {SITES_DIR}. Caddy's `import \
         {SITES_DIR}*{EXTENSION}` does not recurse, so a site there is deployed and \
         never served"
    );
    under
        .strip_suffix(EXTENSION)
        .unwrap_or_else(|| {
            panic!(
                "{id} writes `{under}`, which caddy's `import {SITES_DIR}*{EXTENSION}` \
                 does not match, so the site is deployed and never served"
            )
        })
        .to_string()
}

// ── The premise ───────────────────────────────────────────────────────────

/// The bytes the caddy role installs as [`MAIN_CONFIG`], found through the
/// task that installs them.
///
/// Through the task, not by path, for the reason [`vhosts`] is: the role
/// `copy`s a static file today, a `template` would read identically, and its
/// own `templates/Caddyfile.j2` is referenced by nothing. Hard-coding either
/// leaves this fence able to read the file that is not the one deployed.
fn main_config() -> String {
    for task in role_tasks("caddy") {
        let Some(args) = ["ansible.builtin.copy", "ansible.builtin.template"]
            .iter()
            .find_map(|module| field(&task.body, module))
            .and_then(Value::as_mapping)
        else {
            continue;
        };
        if field(args, "dest").and_then(Value::as_str) != Some(MAIN_CONFIG) {
            continue;
        }
        let src = field(args, "src")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("the task installing {MAIN_CONFIG} must name a src"));
        let path = role_dir("caddy").join(src);
        return fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", relative(&path)));
    }
    panic!(
        "no task in the caddy role installs {MAIN_CONFIG}; every assertion in this file argues from what that file imports"
    )
}

/// Everything below reads a filename and argues about whether caddy will serve
/// it. That argument rests on one line of caddy's own config, and the whole
/// file would go on passing if the line changed — against a glob that no
/// longer exists.
///
/// So the glob is read, not assumed. Widen it to recurse and
/// `test_every_vhost_file_is_named_for_the_app_that_serves_it`'s hard stop on
/// a subdirectory becomes a rule with no reason; change the extension and
/// every message here names a pattern caddy ignores.
#[test]
fn test_the_import_glob_this_fence_argues_from_is_the_one_caddy_reads() {
    let glob = format!("import {SITES_DIR}*{EXTENSION}");
    let installed = main_config();
    assert!(
        installed.contains(&glob),
        "caddy's config no longer holds `{glob}`, which every assertion in this file \
         reasons from: that one line is why a vhost's filename decides whether it is \
         served at all. Re-read it against what {MAIN_CONFIG} imports now:\n{installed}"
    );
}

// ── The reach ─────────────────────────────────────────────────────────────

/// The counts, which `vhost_acme_token.rs` also states: one vhost found would
/// satisfy every assertion below and check one file.
#[test]
fn test_the_fence_reads_every_vhost_in_the_tree() {
    let found = vhosts();
    let roles: BTreeSet<&str> = found.iter().map(|vhost| vhost.role.as_str()).collect();

    assert_eq!(
        found.len(),
        18,
        "the tree deploys a different number of caddy sites than this fence was \
         written against; check the new one's filename, then update this count \
         here and in vhost_acme_token.rs: {:?}",
        found.iter().map(|vhost| vhost.id()).collect::<Vec<_>>()
    );
    assert_eq!(
        roles.len(),
        17,
        "17 roles serve those 18 sites — colporteur serves two: {roles:?}"
    );
}

/// And the same domain against the discovery that does not read a `template`
/// task at all.
///
/// A role notifies `Restart caddy` because it wrote a site. If this walk found
/// none for it, the walk is not reading the tree the fleet deploys — which is
/// the failure a count cannot report, because the count would have been
/// updated to match the narrowed walk.
#[test]
fn test_every_role_that_can_restart_caddy_deploys_a_site_this_fence_reads() {
    let serving: BTreeSet<String> = vhosts().into_iter().map(|vhost| vhost.role).collect();
    let restarting: BTreeSet<String> = roles_that_restart_caddy()
        .into_iter()
        .filter(|role| role != OWNS_THE_PROCESS)
        .collect();

    assert!(
        !restarting.is_empty(),
        "discovery found no role that notifies a caddy restart besides \
         {OWNS_THE_PROCESS}; both halves of this fence's domain would be empty"
    );
    assert_eq!(
        restarting,
        serving,
        "a role that restarts caddy writes a site, and a role that writes a site \
         restarts caddy. Left over on the restart side: {:?}. Left over on the vhost \
         side: {:?}",
        restarting.difference(&serving).collect::<Vec<_>>(),
        serving.difference(&restarting).collect::<Vec<_>>()
    );
}

// ── The filename is an identity ───────────────────────────────────────────

/// The defect itself: a `dest` the FQDN reaches into.
#[test]
fn test_no_vhost_is_written_to_a_path_an_expression_composes() {
    for vhost in vhosts() {
        assert!(
            !vhost.dest.contains("{{"),
            "{} writes `{}`. A filename composed off a name the App serves moves when \
             that name moves, and nothing removes the file left behind — so a subdomain \
             or Zone change writes a second site beside the first and caddy serves both \
             (#954). Write the App's own name literally; the FQDN belongs on the site \
             line inside the file",
            vhost.id(),
            vhost.dest
        );
    }
}

/// And the filename is the App's name, not merely some literal. A literal that
/// says nothing about its owner leaves the operator doing the sweep below with
/// no way to tell whose file is whose.
#[test]
fn test_every_vhost_file_is_named_for_the_app_that_serves_it() {
    for vhost in vhosts() {
        let id = vhost.id();
        let stem = stem(&vhost);
        let app = app_of(&vhost.role)
            .unwrap_or_else(|| panic!("{id} serves a site for no App this walk can name"));
        assert!(
            stem == app || stem.starts_with(&format!("{app}-")),
            "{id} writes `{stem}{EXTENSION}`, which names no App. Name it \
             `{app}{EXTENSION}`, or `{app}-<what it serves>{EXTENSION}` for a second site"
        );
    }
}

/// Two roles cannot hold one public name, so the old paths could not collide.
/// Two hand-written literals can, and the loser is overwritten on whichever
/// role runs second — a site that vanishes on deploy order alone.
#[test]
fn test_no_two_vhosts_write_the_same_file() {
    let mut by_dest: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for vhost in vhosts() {
        by_dest
            .entry(vhost.dest.clone())
            .or_default()
            .push(vhost.id());
    }
    let clashes: Vec<(&String, &Vec<String>)> =
        by_dest.iter().filter(|(_, who)| who.len() > 1).collect();
    assert!(
        clashes.is_empty(),
        "two sites are deployed to one path, so whichever role runs second wins and \
         the other App goes dark: {clashes:?}"
    );
}

/// The one-time sweep's pattern must never match a live site.
///
/// The old naming left one file per Host per name ever served, and a role
/// cannot know which of them were once its own — so clearing them is an
/// operator step, `find /etc/caddy/sites -name '*.*.caddyfile' -delete`. That
/// pattern is safe only while every live site's name is dot-free, which is
/// true of every App name and is asserted here rather than left as a property
/// of the App names that happen to exist.
#[test]
fn test_no_vhost_file_matches_the_operators_sweep_pattern() {
    for vhost in vhosts() {
        let id = vhost.id();
        let stem = stem(&vhost);
        assert!(
            !stem.contains('.'),
            "{id} writes `{stem}{EXTENSION}`, which \
             `find {SITES_DIR} -name '*.*{EXTENSION}' -delete` would delete. That sweep \
             clears the FQDN-named files the old naming left on each Host, and it tells \
             them apart from a live site by the dot alone"
        );
    }
}
