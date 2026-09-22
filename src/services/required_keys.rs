use crate::config::{Config, Preflight};
use crate::key_registry::KeyRegistry;
use crate::playbook_meta::PlaybookMeta;
use crate::services::dependency_resolver::parse_roster;
use crate::services::zone;
use eyre::Result;
use std::path::{Path, PathBuf};

const META_SUFFIX: &str = ".meta.yml";
const REGISTRY_FILE: &str = "keys.yml";
const PLAYBOOKS_DIR: &str = "playbooks";

/// The `required_keys` one Playbook Meta declares, empty when the App has no
/// Meta. Every name is checked against the Key Registry here, so a typo fails
/// where the declaration is read rather than mid-play.
fn declared_keys(playbooks_dir: &Path, stem: &str, registry: &KeyRegistry) -> Result<Vec<String>> {
    let path = playbooks_dir.join(format!("{stem}{META_SUFFIX}"));
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let keys = PlaybookMeta::load(&path)?.required_keys;
    for key in &keys {
        if registry.get(key).is_none() {
            eyre::bail!(
                "{} declares required key '{key}', which is absent from the Key Registry",
                path.display()
            );
        }
    }
    Ok(keys)
}

/// The roster roles a run enters.
///
/// A tagged run selects a role when one of its declared tags was named, or when
/// the tag is the role's own name. An untagged run enters the whole roster, so
/// it selects every entry that is not `when:`-guarded — a guard turns on Host
/// facts no caller can evaluate before the play runs, and demanding its keys
/// would fail Hosts the role never touches.
fn selected_roles(playbook_path: &Path, tags: &[String]) -> Result<Vec<String>> {
    if !playbook_path.is_file() {
        return Ok(Vec::new());
    }
    Ok(parse_roster(playbook_path)?
        .into_iter()
        .filter(|role| match tags {
            [] => !role.guarded,
            tags => tags
                .iter()
                .any(|tag| *tag == role.name || role.tags.contains(tag)),
        })
        .map(|role| role.name)
        .collect())
}

/// A Playbook name with its extension trimmed, however it was spelled.
fn playbook_stem(playbook: &str) -> &str {
    playbook
        .strip_suffix(".yml")
        .or_else(|| playbook.strip_suffix(".yaml"))
        .unwrap_or(playbook)
}

/// The Playbook file for `stem`, whichever extension it carries.
fn roster_path(playbooks_dir: &Path, stem: &str) -> PathBuf {
    let yml = playbooks_dir.join(format!("{stem}.yml"));
    if yml.is_file() {
        return yml;
    }
    playbooks_dir.join(format!("{stem}.yaml"))
}

/// Everything a run reads a Playbook Meta off: the Playbook itself, plus the
/// roster roles its tags select. The one list `required_keys_for` unions keys
/// over and `preflight_for` demands Zones over, so the two cannot disagree
/// about what the run is made of.
fn run_sources(playbooks_dir: &Path, stem: &str, tags: Option<&[String]>) -> Result<Vec<String>> {
    let mut sources = vec![stem.to_string()];
    sources.extend(selected_roles(
        &roster_path(playbooks_dir, stem),
        tags.unwrap_or_default(),
    )?);
    Ok(sources)
}

/// The effective required config keys for one Playbook run: the Playbook's own
/// Meta declarations unioned with the Metas of the roles the selected tags
/// resolve to.
///
/// An untagged run resolves no roles: the roster's `when:`-guarded roles do not
/// run on every Host, so unioning the whole roster would demand keys the run
/// never reads.
pub fn required_keys_for(
    ansible_dir: &Path,
    playbook: &str,
    tags: Option<&[String]>,
) -> Result<Vec<String>> {
    let playbooks_dir = ansible_dir.join(PLAYBOOKS_DIR);
    let registry = KeyRegistry::load(&ansible_dir.join(REGISTRY_FILE))?;
    let stem = playbook_stem(playbook);

    let mut keys: Vec<String> = Vec::new();
    for source in run_sources(&playbooks_dir, stem, tags)? {
        for key in declared_keys(&playbooks_dir, &source, &registry)? {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    Ok(keys)
}

/// Whether a run over `playbook` with `tags` enters `role`.
///
/// The same role selection Preflight resolves keys through, asked as a
/// question — same [`playbook_stem`], same [`roster_path`], same
/// [`selected_roles`] — so a caller gating work on "does this run reach role
/// X" and the Preflight demanding X's keys cannot disagree about which roles a
/// run enters. #768's auto-mint is that caller: minting a pre-auth key costs an
/// SSH round trip to the coordinator, and only a run entering the enrolling
/// role can consume one.
pub fn run_enters_role(
    ansible_dir: &Path,
    playbook: &str,
    tags: Option<&[String]>,
    role: &str,
) -> Result<bool> {
    let playbooks_dir = ansible_dir.join(PLAYBOOKS_DIR);
    let stem = playbook_stem(playbook);
    Ok(
        selected_roles(&roster_path(&playbooks_dir, stem), tags.unwrap_or_default())?
            .iter()
            .any(|selected| selected == role),
    )
}

/// Build a [`Preflight`] for `playbook`, validating every key the Playbook
/// Metas declare for this run. Every production path to an Ansible run comes
/// through here, so the Metas are the only authority a deploy consults.
///
/// `ansible_dir` is the caller's already-prepared Assets Tree: a deploy
/// preflights every run in its plan, and preparing the tree per run would take
/// the extract-and-sweep lock once per playbook instead of once (ADR-0034).
pub fn preflight_for(
    config: &Config,
    ansible_dir: &Path,
    playbook: &str,
    tags: Option<&[String]>,
    host: &str,
) -> Result<Preflight> {
    let known: Vec<String> = crate::services::inventory::get_hosts(None, None)?
        .into_iter()
        .map(|h| h.name)
        .collect();
    assert_host_overrides_known(config, &known)?;
    zone::assert_no_fleet_wide_zone(config)?;
    assert_zones_resolve(ansible_dir, config, playbook, tags, host)?;
    let computed =
        zone::computed_vars(&all_metas(&ansible_dir.join(PLAYBOOKS_DIR))?, config, host)?;
    Ok(config
        .preflight_with_keys(&required_keys_for(ansible_dir, playbook, tags)?, Some(host))?
        .with_computed_vars(computed))
}

/// Every App with a Playbook Meta, whatever this run deploys.
///
/// The Computed Vars cover all of them on purpose: blocky builds its
/// `customDNS` map `run_once` over every Meta, so a map narrowed to the run's
/// Apps is a name the tailnet stops resolving the next time one App is
/// deployed alone (ADR-0081). A Meta that fails to parse is skipped rather
/// than fatal — the run's own Metas are parsed strictly above, and a
/// deploy of one App has no business failing on a sibling's syntax.
fn all_metas(playbooks_dir: &Path) -> Result<Vec<(String, PlaybookMeta)>> {
    let Ok(entries) = std::fs::read_dir(playbooks_dir) else {
        return Ok(Vec::new());
    };
    let mut metas: Vec<(String, PlaybookMeta)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let app = path
                .file_name()?
                .to_str()?
                .strip_suffix(META_SUFFIX)?
                .to_string();
            Some((app, PlaybookMeta::load(&path).ok()?))
        })
        .collect();
    metas.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(metas)
}

/// Every App this run deploys has its effective **Zone**'s pair answered for
/// the Host it is about to run against (ADR-0081).
///
/// This is what the App Metas declaring `domain` and
/// `cloudflare_dns_api_token` used to say, generalised: an App in a Zone of
/// its own demands *that* Zone's pair, and the fleet's Apps go on demanding
/// the fleet's. It lives here rather than in [`required_keys_for`] on
/// purpose — the answer depends on `Config` and on the target Host, and
/// `required_keys_for` is a pure function of `(playbook, tags)` that the
/// `Preflight` type's guarantee rests on.
///
/// Only an App that **publishes a name** is asked. A bot, a runtime or a
/// Composition serves no vhost and writes no record, so demanding a Zone of
/// one would put a Zone's token on a Host that needs none — which is the
/// outcome ADR-0068 exists to prevent, arrived at from the other side.
fn assert_zones_resolve(
    ansible_dir: &Path,
    config: &Config,
    playbook: &str,
    tags: Option<&[String]>,
    host: &str,
) -> Result<()> {
    let playbooks_dir = ansible_dir.join(PLAYBOOKS_DIR);
    let stem = playbook_stem(playbook);
    for app in run_sources(&playbooks_dir, stem, tags)? {
        let path = playbooks_dir.join(format!("{app}{META_SUFFIX}"));
        if !path.is_file() {
            continue;
        }
        let meta = PlaybookMeta::load(&path)?;
        if !zone::publishes_a_name(&meta, config, &app, Some(host)) {
            continue;
        }
        let zone = zone::effective_zone(&meta, config, &app, Some(host))?;
        zone::resolve(&zone, config, Some(host))
            .map_err(|e| eyre::eyre!("{app} cannot be deployed: {e}"))?;
    }
    Ok(())
}

/// Every `[hosts.<name>]` table must name a Host the roster knows: a typoed
/// name is a fail-open — the run proceeds on the fleet-wide answers the table
/// meant to withdraw (ADR-0058).
pub(crate) fn assert_host_overrides_known(config: &Config, known: &[String]) -> Result<()> {
    for name in config.host_override_names() {
        if !known.contains(&name) {
            eyre::bail!(
                "[hosts.{name}] in config.toml names no known host (known: {})",
                known.join(", ")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn repo_ansible_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible")
    }

    /// An ansible dir holding a Key Registry of `keys` plus the given
    /// `<name>` → file-body pairs under `playbooks/`.
    fn fixture_ansible_dir(keys: &[&str], files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = String::from("keys:\n");
        for key in keys {
            registry.push_str(&format!("  {key}:\n    secret: false\n    doc: test key\n"));
        }
        std::fs::write(dir.path().join(REGISTRY_FILE), registry).unwrap();
        let playbooks = dir.path().join(PLAYBOOKS_DIR);
        std::fs::create_dir_all(&playbooks).unwrap();
        for (name, body) in files {
            std::fs::write(playbooks.join(name), body).unwrap();
        }
        dir
    }

    // ── union semantics ───────────────────────────────────────────────────────

    #[test]
    fn test_untagged_run_unions_every_unguarded_roster_role() {
        let dir = fixture_ansible_dir(
            &["admin_user_name", "app_token"],
            &[
                ("apps.meta.yml", "required_keys: [admin_user_name]\n"),
                ("app.meta.yml", "required_keys: [app_token]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: app\n      tags: [apps, app]\n",
                ),
            ],
        );
        let keys = required_keys_for(dir.path(), "apps.yml", None).unwrap();
        assert_eq!(
            keys,
            vec!["admin_user_name".to_string(), "app_token".to_string()]
        );
    }

    #[test]
    fn test_untagged_run_skips_a_when_guarded_role() {
        let dir = fixture_ansible_dir(
            &["admin_user_name", "guarded_token", "plain_token"],
            &[
                ("apps.meta.yml", "required_keys: [admin_user_name]\n"),
                ("guarded.meta.yml", "required_keys: [guarded_token]\n"),
                ("plain.meta.yml", "required_keys: [plain_token]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: guarded\n      tags: [apps, guarded]\n      when: \"'x' in group_names\"\n    - role: plain\n      tags: [apps, plain]\n",
                ),
            ],
        );
        let keys = required_keys_for(dir.path(), "apps.yml", None).unwrap();
        assert_eq!(
            keys,
            vec!["admin_user_name".to_string(), "plain_token".to_string()]
        );
    }

    /// Naming the guarded role's tag is the operator asserting the role runs,
    /// so its keys are demanded — only the untagged sweep skips it.
    #[test]
    fn test_naming_a_guarded_roles_tag_still_demands_its_keys() {
        let dir = fixture_ansible_dir(
            &["admin_user_name", "guarded_token"],
            &[
                ("apps.meta.yml", "required_keys: [admin_user_name]\n"),
                ("guarded.meta.yml", "required_keys: [guarded_token]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: guarded\n      tags: [apps, guarded]\n      when: \"'x' in group_names\"\n",
                ),
            ],
        );
        let tags = vec!["guarded".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert!(keys.contains(&"guarded_token".to_string()), "{keys:?}");
    }

    #[test]
    fn test_tagged_run_unions_the_selected_roles_meta() {
        let dir = fixture_ansible_dir(
            &["admin_user_name", "app_token"],
            &[
                ("apps.meta.yml", "required_keys: [admin_user_name]\n"),
                ("app.meta.yml", "required_keys: [app_token]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: app\n      tags: [apps, app]\n",
                ),
            ],
        );
        let tags = vec!["app".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert_eq!(
            keys,
            vec!["admin_user_name".to_string(), "app_token".to_string()]
        );
    }

    #[test]
    fn test_category_tag_unions_every_role_carrying_it() {
        let dir = fixture_ansible_dir(
            &["base", "one_key", "two_key", "three_key"],
            &[
                ("apps.meta.yml", "required_keys: [base]\n"),
                ("one.meta.yml", "required_keys: [one_key]\n"),
                ("two.meta.yml", "required_keys: [two_key]\n"),
                ("three.meta.yml", "required_keys: [three_key]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: one\n      tags: [apps, media, one]\n    - role: two\n      tags: [apps, media, two]\n    - role: three\n      tags: [apps, web, three]\n",
                ),
            ],
        );
        let tags = vec!["media".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert_eq!(
            keys,
            vec![
                "base".to_string(),
                "one_key".to_string(),
                "two_key".to_string()
            ]
        );
        assert!(!keys.contains(&"three_key".to_string()));
    }

    #[test]
    fn test_union_deduplicates_keys_declared_twice() {
        let dir = fixture_ansible_dir(
            &["domain"],
            &[
                ("apps.meta.yml", "required_keys: [domain]\n"),
                ("app.meta.yml", "required_keys: [domain]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: app\n      tags: [apps, app]\n",
                ),
            ],
        );
        let tags = vec!["app".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert_eq!(keys, vec!["domain".to_string()]);
    }

    #[test]
    fn test_role_without_a_meta_contributes_nothing() {
        let dir = fixture_ansible_dir(
            &["base"],
            &[
                ("apps.meta.yml", "required_keys: [base]\n"),
                (
                    "apps.yml",
                    "---\n- hosts: all\n  roles:\n    - role: metaless\n      tags: [apps, metaless]\n",
                ),
            ],
        );
        let tags = vec!["metaless".to_string()];
        let keys = required_keys_for(dir.path(), "apps.yml", Some(&tags)).unwrap();
        assert_eq!(keys, vec!["base".to_string()]);
    }

    #[test]
    fn test_standalone_playbook_reads_its_own_meta() {
        let dir = fixture_ansible_dir(
            &["solo_token"],
            &[("solo.meta.yml", "required_keys: [solo_token]\n")],
        );
        let keys = required_keys_for(dir.path(), "solo.yml", None).unwrap();
        assert_eq!(keys, vec!["solo_token".to_string()]);
    }

    #[test]
    fn test_playbook_without_a_meta_requires_nothing() {
        let dir = fixture_ansible_dir(&["unused"], &[]);
        let keys = required_keys_for(dir.path(), "ghost.yml", None).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_unknown_key_name_in_a_meta_is_rejected() {
        let dir = fixture_ansible_dir(
            &["known"],
            &[("solo.meta.yml", "required_keys: [known, typoed_key]\n")],
        );
        let err = required_keys_for(dir.path(), "solo.yml", None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("typoed_key"), "{msg}");
        assert!(msg.contains("Key Registry"), "{msg}");
    }

    // ── the repo's own Metas ──────────────────────────────────────────────────

    #[test]
    fn test_repo_apps_playbook_resolves_the_base_keys_untagged() {
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert!(set.contains("admin_user_name"), "{keys:?}");
    }

    /// `domain` and `cloudflare_dns_api_token` left `apps.meta.yml` with
    /// ADR-0081 and are demanded as the fleet **Zone**'s pair instead. Here so
    /// the move reads as a move: this function is the pure half, and dropping
    /// the pair from it without the Zone half would be a silent withdrawal.
    #[test]
    fn test_the_base_domain_keys_are_no_longer_a_declared_requirement() {
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", None).unwrap();
        for key in ["domain", "cloudflare_dns_api_token"] {
            assert!(
                !keys.iter().any(|k| k == key),
                "{key} is the fleet Zone's, demanded by assert_zones_resolve: {keys:?}"
            );
        }
    }

    /// The run enters every unguarded App, so it demands their keys too — this
    /// is the case the drift left failing mid-play.
    #[test]
    fn test_repo_untagged_apps_run_demands_the_app_keys() {
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        for key in [
            "yourls_cookiekey",
            "paperless_secret_key",
            "grimmory_db_password",
            "radio_listener_password",
            "immich_db_password",
        ] {
            assert!(
                set.contains(key),
                "untagged apps should demand {key}: {keys:?}"
            );
        }
    }

    /// `hermes` is the roster's one `when:`-guarded App: it runs only on Hosts
    /// in the hermes group, so an untagged run cannot demand its keys.
    #[test]
    fn test_repo_untagged_apps_run_skips_the_guarded_app() {
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", None).unwrap();
        assert!(
            !keys.iter().any(|k| k == "hermes_llm_api_key"),
            "untagged apps must not demand a guarded App's keys: {keys:?}"
        );
    }

    /// `blocky` and `headscale` carry infrastructure's `when:` gates
    /// (ADR-0051, ADR-0058): whether they run is the target Host's answer, so
    /// an untagged run cannot demand their keys.
    #[test]
    fn test_repo_untagged_infrastructure_run_skips_the_guarded_roles() {
        let keys = required_keys_for(&repo_ansible_dir(), "infrastructure.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert!(set.contains("admin_user_name"), "{keys:?}");
        for gate in ["blocky_subdomain", "headscale_subdomain"] {
            assert!(
                !set.contains(gate),
                "untagged infrastructure must not demand a guarded role's key: {keys:?}"
            );
        }
    }

    /// The gate reads `headscale_subdomain` from config alone (#710), so a run
    /// that names the tag without the key would skip every task it asked for.
    /// Naming the tag is the operator asserting the role runs, and then they
    /// are asked. Any selecting tag counts, category tags included — ADR-0045's
    /// selection rule, pinned here so a change to it is deliberate.
    #[test]
    fn test_repo_headscale_tag_demands_the_gate_key() {
        for tag in ["headscale", "vpn"] {
            let tags = vec![tag.to_string()];
            let keys =
                required_keys_for(&repo_ansible_dir(), "infrastructure.yml", Some(&tags)).unwrap();
            assert!(
                keys.iter().any(|k| k == "headscale_subdomain"),
                "-t {tag} must demand headscale_subdomain: {keys:?}"
            );
        }
    }

    #[test]
    fn test_repo_blocky_tag_demands_the_gate_key() {
        for tag in ["blocky", "dns"] {
            let tags = vec![tag.to_string()];
            let keys =
                required_keys_for(&repo_ansible_dir(), "infrastructure.yml", Some(&tags)).unwrap();
            assert!(
                keys.iter().any(|k| k == "blocky_subdomain"),
                "-t {tag} must demand blocky_subdomain: {keys:?}"
            );
        }
    }

    #[test]
    fn test_repo_bootstrap_keys_come_from_its_meta() {
        let keys = required_keys_for(&repo_ansible_dir(), "bootstrap.yml", None).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert_eq!(set, HashSet::from(["admin_user_name", "ssh_port"]));
    }

    /// Every key the audit in ADR-0045 found an App role to hard-require: an
    /// in-role `assert` names it, or it is referenced unguarded with no default
    /// in the role, group_vars, or anywhere else. Enforced at Preflight now,
    /// where before the run failed mid-play.
    const APP_SPECIFIC_KEYS: &[(&str, &[&str])] = &[
        (
            "baikal",
            &[
                "admin_user_email",
                "baikal_admin_password",
                "baikal_busy_feed_token",
                "baikal_subdomain",
            ],
        ),
        (
            "bichon",
            &["bichon_api_token", "bichon_encryption_password"],
        ),
        ("calibre", &["calibre_subdomain"]),
        (
            "colporteur",
            &["colporteur_feeds_password", "colporteur_subdomain"],
        ),
        ("freshrss", &["freshrss_subdomain"]),
        (
            "gokapi",
            &[
                "gokapi_admin_password",
                "gokapi_admin_user",
                "gokapi_subdomain",
            ],
        ),
        (
            "grimmory",
            &[
                "grimmory_admin_password",
                "grimmory_admin_user",
                "grimmory_db_password",
                "grimmory_subdomain",
            ],
        ),
        (
            "immich",
            &[
                "immich_b2_application_key",
                "immich_b2_key_id",
                "immich_db_password",
                "immich_restic_password",
                "immich_restic_repository",
                "immich_subdomain",
            ],
        ),
        ("navidrome", &["navidrome_subdomain"]),
        (
            "paperless",
            &[
                "admin_user_email",
                "paperless_admin_password",
                "paperless_admin_user",
                "paperless_db_password",
                "paperless_secret_key",
            ],
        ),
        ("radio", &["radio_listener_password", "radio_subdomain"]),
        ("tgtg", &["tgtg_telegram_bot_token"]),
        (
            "yourls",
            &[
                "yourls_admin_password",
                "yourls_admin_user",
                "yourls_cookiekey",
                "yourls_db_password",
                "yourls_subdomain",
            ],
        ),
    ];

    #[test]
    fn test_app_tag_resolves_every_key_its_role_requires() {
        let mut gaps = Vec::new();
        for (app, expected) in APP_SPECIFIC_KEYS {
            let tags = vec![(*app).to_string()];
            let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", Some(&tags)).unwrap();
            for key in *expected {
                if !keys.iter().any(|k| k == key) {
                    gaps.push(format!("{app}: {key}"));
                }
            }
        }
        assert!(
            gaps.is_empty(),
            "app tags that do not resolve a key their role requires: {}",
            gaps.join(", ")
        );
    }

    #[test]
    fn test_app_tag_still_resolves_the_shared_base_keys() {
        let tags = vec!["colporteur".to_string()];
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", Some(&tags)).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        assert!(set.contains("admin_user_name"), "{keys:?}");
    }

    #[test]
    fn test_category_tag_unions_the_apps_beneath_it() {
        let tags = vec!["media".to_string()];
        let keys = required_keys_for(&repo_ansible_dir(), "apps.yml", Some(&tags)).unwrap();
        let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
        for key in [
            "radio_listener_password",
            "navidrome_subdomain",
            "immich_db_password",
        ] {
            assert!(set.contains(key), "media should resolve {key}: {keys:?}");
        }
        assert!(
            !set.contains("yourls_cookiekey"),
            "media must not resolve a web App's key: {keys:?}"
        );
    }

    /// An App that is also a standalone playbook cannot lean on
    /// `apps.meta.yml` for the shared base, and since ADR-0081 neither can
    /// lean on it for the domain pair — the Zone demand reaches it through
    /// its own Meta's name either way.
    #[test]
    fn test_standalone_app_playbook_resolves_its_own_base_keys() {
        for app in ["gokapi", "immich"] {
            let keys = required_keys_for(&repo_ansible_dir(), &format!("{app}.yml"), None).unwrap();
            let set: HashSet<&str> = keys.iter().map(String::as_str).collect();
            assert!(
                set.contains(&*format!("{app}_subdomain")),
                "{app}: {keys:?}"
            );
        }
    }

    #[test]
    fn test_repo_hardening_requires_nothing() {
        let keys = required_keys_for(&repo_ansible_dir(), "hardening.yml", None).unwrap();
        assert!(keys.is_empty(), "{keys:?}");
    }

    #[test]
    fn test_host_override_tables_must_name_known_hosts() {
        let config = Config::from_toml_str(
            r#"
            [hosts.agentbox]
            headscale_subdomain = ""
        "#,
        )
        .unwrap();
        let known = vec!["auberge".to_string(), "agent-box".to_string()];
        let err = assert_host_overrides_known(&config, &known).unwrap_err();
        assert!(err.to_string().contains("agentbox"), "{err}");

        let config = Config::from_toml_str(
            r#"
            [hosts.agent-box]
            headscale_subdomain = ""
        "#,
        )
        .unwrap();
        assert!(assert_host_overrides_known(&config, &known).is_ok());
    }

    // ── the zone demand ───────────────────────────────────────────────────────

    /// An ansible dir whose `apps.yml` roster holds one App per `(name, meta)`
    /// pair, each selectable by its own name as a tag.
    fn zone_fixture(apps: &[(&str, &str)]) -> tempfile::TempDir {
        let mut roster = String::from("---\n- hosts: all\n  roles:\n");
        let mut files = vec![("apps.meta.yml", "required_keys: []\n".to_string())];
        for (name, meta) in apps {
            roster.push_str(&format!("    - role: {name}\n      tags: [apps, {name}]\n"));
            files.push((
                Box::leak(format!("{name}.meta.yml").into_boxed_str()) as &str,
                (*meta).to_string(),
            ));
        }
        files.push(("apps.yml", roster));
        let borrowed: Vec<(&str, &str)> = files.iter().map(|(n, b)| (*n, b.as_str())).collect();
        fixture_ansible_dir(&[], &borrowed)
    }

    const FLEET_ANSWERED: &str = r#"
        domain = "fleet.example"
        cloudflare_dns_api_token = "fleet-token"
    "#;

    /// The demand `domain` and `cloudflare_dns_api_token` left the Metas for:
    /// a name-publishing App with no Zone of its own needs the fleet's pair.
    #[test]
    fn test_a_published_app_demands_the_fleet_pair_when_it_names_no_zone() {
        let dir = zone_fixture(&[("navidrome", "required_keys: []\nsubdomain: navidrome\n")]);
        let config = Config::from_toml_str(FLEET_ANSWERED).unwrap();
        assert!(assert_zones_resolve(dir.path(), &config, "apps.yml", None, "auberge").is_ok());

        let bare = Config::from_toml_str("domain = \"fleet.example\"\n").unwrap();
        let err = assert_zones_resolve(dir.path(), &bare, "apps.yml", None, "auberge")
            .unwrap_err()
            .to_string();
        assert!(err.contains("navidrome"), "names the app: {err}");
        assert!(err.contains("cloudflare_dns_api_token"), "{err}");
    }

    /// An App the operator moved demands *its* Zone's pair, and stops
    /// demanding the fleet's — the whole point of the pair being named.
    #[test]
    fn test_an_off_zone_app_demands_its_own_pair_and_not_the_fleets() {
        let dir = zone_fixture(&[("forgejo", "required_keys: []\nsubdomain: git\n")]);
        let config = Config::from_toml_str(
            r#"
            studio_domain = "studio.example"
            studio_cloudflare_dns_api_token = "studio-token"

            [hosts.auberge]
            forgejo_zone = "studio"
        "#,
        )
        .unwrap();
        assert!(assert_zones_resolve(dir.path(), &config, "apps.yml", None, "auberge").is_ok());

        // …and the fleet's pair is still what the same App needs elsewhere.
        let err = assert_zones_resolve(dir.path(), &config, "apps.yml", None, "lechuck")
            .unwrap_err()
            .to_string();
        assert!(err.contains("domain"), "{err}");
    }

    /// An App with no name has no vhost and no record. Demanding a Zone of one
    /// would put a Zone's token on a Host that serves nothing — ADR-0068's
    /// outcome, reached from the other side.
    #[test]
    fn test_an_app_publishing_no_name_demands_no_zone() {
        let dir = zone_fixture(&[("tgtg", "required_keys: []\n")]);
        let empty = Config::from_toml_str("").unwrap();
        assert!(assert_zones_resolve(dir.path(), &empty, "apps.yml", None, "auberge").is_ok());
    }

    /// A Composition and a bootstrap have Metas but deploy no name, so the
    /// demand must not reach them — a virgin Host has no zone answered yet.
    #[test]
    fn test_the_repo_bootstrap_run_demands_no_zone() {
        let empty = Config::from_toml_str("").unwrap();
        for playbook in ["bootstrap.yml", "hardening.yml", "ruche.yml"] {
            assert!(
                assert_zones_resolve(&repo_ansible_dir(), &empty, playbook, None, "auberge")
                    .is_ok(),
                "{playbook} must demand no zone"
            );
        }
    }

    /// The repo's own Apps, through the real Metas: an untagged apps run needs
    /// the fleet pair, and a Host that answers neither is told so.
    #[test]
    fn test_the_repo_untagged_apps_run_demands_the_fleet_pair() {
        let answered = Config::from_toml_str(FLEET_ANSWERED).unwrap();
        assert!(
            assert_zones_resolve(&repo_ansible_dir(), &answered, "apps.yml", None, "auberge")
                .is_ok()
        );

        let empty = Config::from_toml_str("").unwrap();
        let err = assert_zones_resolve(&repo_ansible_dir(), &empty, "apps.yml", None, "auberge")
            .unwrap_err()
            .to_string();
        assert!(err.contains("the fleet's zone"), "{err}");
    }

    /// The agent tier still composes against its own pair, under ADR-0071's
    /// `domain_key:` spelling. A resolver blind to it would demand the parent
    /// domain's token on the one Host that must never hold it.
    #[test]
    fn test_the_repo_agent_tier_demands_the_agents_pair_not_the_fleets() {
        let config = Config::from_toml_str(
            r#"
            agents_domain = "agents.example"
            agents_cloudflare_dns_api_token = "agents-token"
        "#,
        )
        .unwrap();
        assert!(
            assert_zones_resolve(&repo_ansible_dir(), &config, "aoe.yml", None, "ruche").is_ok(),
            "aoe must resolve on agents_* alone"
        );
    }

    // ── role selection, asked directly ────────────────────────────────────────

    #[test]
    fn test_an_untagged_run_enters_an_unguarded_roster_role() {
        let dir = fixture_ansible_dir(
            &[],
            &[(
                "apps.yml",
                "---\n- hosts: all\n  roles:\n    - role: tailscale\n      tags: [network]\n",
            )],
        );
        assert!(run_enters_role(dir.path(), "apps.yml", None, "tailscale").unwrap());
        assert!(!run_enters_role(dir.path(), "apps.yml", None, "caddy").unwrap());
    }

    #[test]
    fn test_a_tagged_run_enters_only_the_roles_its_tags_select() {
        let dir = fixture_ansible_dir(
            &[],
            &[(
                "apps.yml",
                "---\n- hosts: all\n  roles:\n    - role: tailscale\n      tags: [network]\n    - role: caddy\n      tags: [web]\n",
            )],
        );
        let network = ["network".to_string()];
        let web = ["web".to_string()];
        assert!(run_enters_role(dir.path(), "apps.yml", Some(&network), "tailscale").unwrap());
        assert!(!run_enters_role(dir.path(), "apps.yml", Some(&web), "tailscale").unwrap());
    }

    /// A playbook with no roster file at all — a standalone play — enters no
    /// role, so a caller gating an SSH round trip on this does nothing.
    #[test]
    fn test_a_playbook_with_no_roster_enters_no_role() {
        let dir = fixture_ansible_dir(&[], &[]);
        assert!(!run_enters_role(dir.path(), "absent.yml", None, "tailscale").unwrap());
    }

    /// The gate #768's auto-mint reads: an untagged infrastructure run enters
    /// the tailscale role, so it is the run that may need a pre-auth key.
    #[test]
    fn test_the_repo_infrastructure_run_enters_the_enrolling_role() {
        assert!(
            run_enters_role(
                &repo_ansible_dir(),
                "infrastructure.yml",
                None,
                crate::commands::headscale::ENROLLING_ROLE,
            )
            .unwrap()
        );
    }

    /// …and an unrelated run does not, so a routine app deploy costs no round
    /// trip to the coordinator.
    #[test]
    fn test_the_repo_apps_run_does_not_enter_the_enrolling_role() {
        assert!(
            !run_enters_role(
                &repo_ansible_dir(),
                "apps.yml",
                None,
                crate::commands::headscale::ENROLLING_ROLE,
            )
            .unwrap()
        );
    }
}
