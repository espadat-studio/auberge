//! A **Zone** is declared as a pair, and every pin names one that exists.
//!
//! ADR-0081 made an App's Zone two config keys sharing a prefix —
//! `<zone>_domain` and `<zone>_cloudflare_dns_api_token`. A pair is not a
//! shape the registry can enforce: it is a flat map, so half a Zone parses,
//! scaffolds, and reaches a run exactly as well as a whole one. The half that
//! is missing is discovered at ACME time, on a vhost that already went green.
//!
//! The behaviour that reads these declarations — a Meta pin refusing a Config
//! override, a fleet-wide `<app>_zone` being refused, a Zone resolving for one
//! Host — is unit-tested in `services::zone`, against fixtures that state the
//! config they mean. What no unit test can see is whether the **repo's own**
//! declarations are answerable at all, because a resolver is only ever asked
//! about a Zone somebody wrote down. That is what this reads off the tree.

mod common;

use common::{meta_files, parse_yaml, registry_keys, repo};
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The registry key naming a Zone's apex domain, unprefixed.
const DOMAIN_SUFFIX: &str = "domain";
/// The registry key naming a Zone's Cloudflare token, unprefixed.
const TOKEN_SUFFIX: &str = "cloudflare_dns_api_token";
/// The Config key placing an App in a Zone.
const ZONE_SUFFIX: &str = "_zone";

/// The Zone prefix a key belongs to, or `None` when the key is not that half
/// of a pair. `""` is the fleet's Zone, whose pair is unprefixed.
///
/// `<app>_subdomain` ends in `domain` and is not a Zone's half: every App has
/// one, so a bare suffix match would report twenty half-Zones and the fence
/// would be red for reasons that are not about Zones at all.
fn zone_prefix_of(key: &str, suffix: &str) -> Option<String> {
    if key == suffix {
        return Some(String::new());
    }
    if key.ends_with(&format!("_sub{suffix}")) {
        return None;
    }
    key.strip_suffix(&format!("_{suffix}"))
        .filter(|prefix| !prefix.is_empty())
        .map(str::to_string)
}

/// Every Zone the Key Registry names, and which halves of it are present.
fn registry_zones() -> BTreeMap<String, (bool, bool)> {
    let mut zones: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    for key in registry_keys() {
        if let Some(prefix) = zone_prefix_of(&key, DOMAIN_SUFFIX) {
            zones.entry(prefix).or_default().0 = true;
        }
        if let Some(prefix) = zone_prefix_of(&key, TOKEN_SUFFIX) {
            zones.entry(prefix).or_default().1 = true;
        }
    }
    zones
}

/// Every Meta that pins its App to a Zone, as `(app, prefix)`. The fleet's
/// Zone is the empty prefix, as in the registry.
fn declared_pins() -> Vec<(String, String)> {
    meta_files()
        .into_iter()
        .filter_map(|(app, path)| {
            let prefix = parse_yaml(&path)
                .get("zone")
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            Some((app, prefix))
        })
        .collect()
}

/// The domain this fence reads, asserted non-empty so a narrowed scan cannot
/// pass vacuously. Every assertion below quantifies over one of these three
/// sets, and all three hold over the empty set.
#[test]
fn the_tree_declares_zones_pins_and_a_placement_key() {
    let zones = registry_zones();
    assert!(zones.contains_key(""), "the fleet's Zone: {zones:?}");
    assert!(zones.contains_key("agents"), "agents: {zones:?}");

    let pins = declared_pins();
    assert!(
        pins.iter()
            .any(|(app, prefix)| app == "aoe" && prefix == "agents"),
        "the agent tier is the one App the repo pins, so a scan finding no pin found nothing: \
         {pins:?}"
    );

    assert!(
        registry_keys().iter().any(|k| k.ends_with(ZONE_SUFFIX)),
        "no <app>_zone key in the registry: the operator has no way to place an App at all"
    );
}

/// A Zone is a pair. A registry naming one half ships a domain whose vhost can
/// never answer DNS-01, or a token scoped to a zone nothing composes against —
/// and both parse, scaffold and deploy green (ADR-0068).
#[test]
fn every_registry_zone_declares_both_halves() {
    let incomplete: Vec<String> = registry_zones()
        .into_iter()
        .filter(|(_, (domain, token))| !(*domain && *token))
        .map(|(prefix, (domain, _))| {
            let zone = if prefix.is_empty() {
                "the fleet's zone".to_string()
            } else {
                format!("zone '{prefix}'")
            };
            let missing = if domain {
                format!("{prefix}_{TOKEN_SUFFIX}")
            } else {
                format!("{prefix}_{DOMAIN_SUFFIX}")
            };
            format!("{zone} is missing {missing}")
        })
        .collect();
    assert!(
        incomplete.is_empty(),
        "ansible/keys.yml declares half a Zone: {}",
        incomplete.join("; ")
    );
}

/// A pin names a Zone the operator can actually answer. A prefix with no pair
/// in the registry is a Meta demanding a key `config init` never offers.
#[test]
fn every_meta_pin_names_a_zone_the_registry_holds() {
    let zones = registry_zones();
    let dangling: Vec<String> = declared_pins()
        .into_iter()
        .filter(|(_, prefix)| !zones.contains_key(prefix))
        .map(|(app, prefix)| format!("{app} (zone: '{prefix}')"))
        .collect();
    assert!(
        dangling.is_empty(),
        "playbook metas pin Zones the Key Registry does not declare: {}",
        dangling.join(", ")
    );
}

/// An `<app>_zone` key places an App. One naming no App places nothing, and
/// reads to an operator as a supported knob.
#[test]
fn every_zone_placement_key_names_an_app_with_a_meta() {
    let apps: BTreeSet<String> = meta_files().into_iter().map(|(app, _)| app).collect();
    let orphans: Vec<String> = registry_keys()
        .into_iter()
        .filter_map(|key| {
            let app = key.strip_suffix(ZONE_SUFFIX)?.to_string();
            (!apps.contains(&app)).then_some(key)
        })
        .collect();
    assert!(
        orphans.is_empty(),
        "ansible/keys.yml holds <app>_zone keys for apps with no playbook meta: {}",
        orphans.join(", ")
    );
}

/// A Zone is where an App's **public name** lives, so pinning one on an App
/// that publishes no name declares a token for a vhost that does not exist —
/// and `computed_vars` would land it on the Host anyway.
#[test]
fn every_pinned_app_publishes_a_name() {
    let nameless: Vec<String> = declared_pins()
        .into_iter()
        .filter(|(app, _)| {
            let path = repo()
                .join("ansible")
                .join("playbooks")
                .join(format!("{app}.meta.yml"));
            parse_yaml(&path)
                .get("subdomain")
                .and_then(Value::as_str)
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        })
        .map(|(app, prefix)| format!("{app} (zone '{prefix}')"))
        .collect();
    assert!(
        nameless.is_empty(),
        "these apps pin a Zone but declare no subdomain, so they publish no name in it: {}",
        nameless.join(", ")
    );
}
