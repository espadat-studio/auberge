//! A **Computed Var** has no authoring site, and this is what keeps it that
//! way.
//!
//! The CLI resolves each App's **Zone** once and hands the answer down as
//! `<app>_parent_domain`, `<app>_dns_api_token` and the Host's zone set, so
//! the vhost, the `dns_record` call and the deploy-time verification read one
//! value rather than three expressions a test hopes agree (ADR-0081).
//!
//! That only holds while the names are absent from the Key Registry. A
//! Computed Var is not an Injected Key: an Injected Key is *in* the Registry
//! precisely so a stale `config.toml` value can be overridden (ADR-0063,
//! `tests/injected_keys.rs`), while a resolved Zone has no such value to
//! override. Put one of these names in `keys.yml` and `config set` offers it,
//! `config init` scaffolds it, and the operator has been handed a fourth
//! declaration site for a thing ADR-0081 spent two paragraphs cutting down to
//! one.
//!
//! The collision is not hypothetical: `cloudflare_dns_api_token` — the
//! fleet's own Registry key — already ends in [`DNS_TOKEN_SUFFIX`]. One App
//! named `cloudflare` and the two namespaces meet.

mod common;

use auberge::services::zone::{
    DNS_TOKEN_ENV_SUFFIX, DNS_TOKEN_SUFFIX, HOST_ZONES_VAR, PARENT_DOMAIN_SUFFIX,
};
use common::{meta_files, registry_keys};
use std::collections::BTreeSet;

/// Every Computed Var name a run can produce: three per App with a Meta, plus
/// the Host's zone set.
///
/// Built from the same suffixes `services::zone` composes them from
/// (ADR-0046) over the same Metas `preflight_for` walks, so a suffix that
/// changes or an App that appears is in this set without anyone updating it.
fn computed_var_names() -> BTreeSet<String> {
    let mut names = BTreeSet::from([HOST_ZONES_VAR.to_string()]);
    for (app, _) in meta_files() {
        names.insert(format!("{app}{PARENT_DOMAIN_SUFFIX}"));
        names.insert(format!("{app}{DNS_TOKEN_SUFFIX}"));
        names.insert(format!("{app}{DNS_TOKEN_ENV_SUFFIX}"));
    }
    names
}

/// The domain this fence reads, stated as counts so a narrowed walk cannot
/// pass vacuously. Both drift: an App added or removed moves the first, and
/// the second is two per App plus the Host's set.
///
/// `meta_files` already refuses an empty walk, but "non-empty" is not the
/// reach — a walk that found one Meta would satisfy it and check one App.
#[test]
fn the_fence_reads_every_apps_computed_vars() {
    let apps = meta_files().len();
    let names = computed_var_names();

    assert_eq!(
        apps, 29,
        "the playbooks directory holds a different number of Metas than this fence \
         was written against; check the new one's Computed Vars below, then update \
         this count"
    );
    assert_eq!(
        names.len(),
        apps * 3 + 1,
        "every App contributes {PARENT_DOMAIN_SUFFIX}, {DNS_TOKEN_SUFFIX} and \
         {DNS_TOKEN_ENV_SUFFIX}, and the Host's zone set is one more: {names:?}"
    );
}

/// The invariant. A Computed Var that is also a Registry key is a Computed
/// Var an operator can answer — and `config.toml`'s answer would be the one
/// thing ADR-0081 deleted, back in the tree with a plausible-looking name.
#[test]
fn no_computed_var_is_a_key_registry_key() {
    let registry = registry_keys();
    let collisions: Vec<String> = computed_var_names()
        .into_iter()
        .filter(|name| registry.contains(name))
        .collect();

    assert!(
        collisions.is_empty(),
        "ansible/keys.yml gives {collisions:?} an authoring site, but the CLI computes \
         them: a Computed Var is resolved from an App's Zone and has no config answer \
         to override (ADR-0081). Drop the key, or rename the Computed Var."
    );
}
