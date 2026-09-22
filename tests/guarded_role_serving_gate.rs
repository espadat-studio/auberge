use serde_yaml::Value;
use std::collections::BTreeSet;

#[path = "common/mod.rs"]
mod common;

/// A `when:`-guarded role that publishes a name must leave its serving gate
/// unanswered in its own `defaults/`.
///
/// Preflight demands a guarded role's **Zone** off the config answer to
/// `<app>_subdomain`, never off the Meta's `subdomain:` (ADR-0083). The Meta
/// half is a repo fact — true on every Host — so it cannot tell a Host that
/// serves the App from one the guard skips, and asking it of a guarded role
/// refuses `deploy infrastructure` everywhere the App is not served.
///
/// That leaves the config answer load-bearing in a way a role default
/// silently removes. A default for `<app>_subdomain` satisfies ansible's own
/// gate — `blocky_subdomain is defined and ... | length > 0` reads the merged
/// variables, defaults included — while `zone::serving_gate_answered` reads
/// config alone. The role would then run on a Host Preflight demanded no Zone
/// of, compose its FQDN off an absent `<app>_parent_domain`, and die mid-play
/// on an undefined variable: #960 restored, one App at a time.
///
/// `aoe` carried exactly that default until #960. It was saved by something
/// unrelated — `aoe.meta.yml` and `ruche.meta.yml` are unguarded, so
/// `required_keys_for` demanded the agents pair anyway — which is a
/// coincidence rather than a mechanism, and it holds only while every
/// playbook that rosters the role happens to declare the pair itself.
///
/// Stated as a walk with a reach, not a list of names: a fence naming
/// `blocky` and `headscale` passes untouched when the next guarded App lands.
fn guarded_name_publishing_roles() -> BTreeSet<String> {
    let mut guarded = BTreeSet::new();
    for path in common::playbook_files() {
        let doc = common::parse_yaml(&path);
        let Some(plays) = doc.as_sequence() else {
            continue;
        };
        for play in plays {
            let Some(roles) = play.get("roles").and_then(Value::as_sequence) else {
                continue;
            };
            for entry in roles {
                let Some(name) = entry.get("role").and_then(Value::as_str) else {
                    continue;
                };
                if entry.get("when").is_none() || !publishes_a_name(name) {
                    continue;
                }
                guarded.insert(name.to_string());
            }
        }
    }
    guarded
}

/// Whether the role's App declares a `subdomain:` — the repo-side half of
/// `zone::publishes_a_name`, and the half that makes the Zone demand apply.
fn publishes_a_name(role: &str) -> bool {
    let path = common::playbooks_dir().join(format!("{role}.meta.yml"));
    if !path.is_file() {
        return false;
    }
    common::parse_yaml(&path)
        .get("subdomain")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

#[test]
fn a_guarded_role_that_publishes_a_name_declares_no_subdomain_default() {
    let roles = guarded_name_publishing_roles();
    assert!(
        !roles.is_empty(),
        "no guarded name-publishing role found at all; the roster walk reads \
         ansible/playbooks/*.yml for `roles:` entries carrying a `when:`, and an empty walk \
         makes this fence pass over nothing"
    );

    for role in &roles {
        let gate = format!("{role}_subdomain");
        assert!(
            !common::defaults(role).contains_key(&gate),
            "ansible/roles/{role}/defaults/main.yml answers '{gate}'. The role is `when:`-guarded \
             and publishes a name, so Preflight demands its Zone off the *config* answer to \
             '{gate}' (ADR-0083). A default satisfies ansible's gate without satisfying that \
             demand, so the role runs on a Host that was asked for no Zone and dies on an \
             undefined '{role}_parent_domain' — drop the default and let the Meta's \
             `required_keys` demand the key."
        );
    }
}

/// The reach, so the walk above cannot quietly narrow. Both of
/// `infrastructure.yml`'s gated Substrate Apps and the agent tier's dashboard
/// publish a name behind a guard; a walk that stops seeing one of them is a
/// fence that stopped covering it.
#[test]
fn the_walk_reaches_every_guarded_app_that_publishes_a_name() {
    let roles = guarded_name_publishing_roles();
    let expected: BTreeSet<String> = ["aoe", "blocky", "headscale"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(
        roles, expected,
        "the set of guarded name-publishing roles moved. Add the new one here once you have \
         checked it carries no `<app>_subdomain` default, or drop a name whose guard or \
         `subdomain:` is gone."
    );
}
