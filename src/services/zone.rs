//! An App's **Zone**: the DNS zone its public name lives in, named by the
//! prefix its two Key Registry entries share — `<zone>_domain` and
//! `<zone>_cloudflare_dns_api_token`. The fleet's Zone is the unnamed one
//! (ADR-0081).
//!
//! A Zone is a pair, never a domain alone. A Cloudflare token is zone-scoped,
//! so holding the name without the token is a vhost that can never complete an
//! ACME challenge (ADR-0068). Every lookup here therefore resolves both or
//! neither.
//!
//! Which Zone an App is in has two declaration sites because it is two
//! different claims. A Playbook Meta's `zone:` is the repo asserting the App
//! must be isolated — true for every operator. `<app>_zone` in `Config` is the
//! operator placing an App in a Zone of their own. The repo's pin wins, by
//! refusing the override rather than shadowing it: a silent precedence rule is
//! how an operator discovers ADR-0068's isolation was unset for them.
//!
//! `domain_key:` is ADR-0071's spelling of the same pin, still live until the
//! follow-up re-spells it. [`pinned`] reads both, because a resolver that saw
//! only `zone:` would put the agent tier back in the fleet's Zone and demand
//! the parent domain's token on the one Host ADR-0068 exists to keep it off.

use crate::config::Config;
use crate::playbook_meta::{DEFAULT_DOMAIN_KEY, PlaybookMeta};
use eyre::Result;
use std::collections::BTreeSet;

/// The second half of a Zone's key pair, and the tail every `domain_key:`
/// carries — `agents_domain` is the `agents` Zone said the old way.
const DOMAIN_SUFFIX: &str = DEFAULT_DOMAIN_KEY;

/// The first half of a Zone's key pair, unprefixed.
const TOKEN_SUFFIX: &str = "cloudflare_dns_api_token";

/// The Config key placing an App in a Zone: `<app>_zone`.
const ZONE_SUFFIX: &str = "_zone";

/// A DNS zone, identified by the prefix its Key Registry pair shares. `None`
/// is the fleet's Zone, whose pair is unprefixed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Zone {
    prefix: Option<String>,
}

impl Zone {
    /// The Zone every App is in unless something says otherwise.
    pub fn fleet() -> Self {
        Self { prefix: None }
    }

    pub fn named(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
        }
    }

    /// How an error message spells this Zone. The fleet's has no prefix to
    /// name, and "the `` zone" reads as a bug rather than as the default.
    pub fn label(&self) -> String {
        match &self.prefix {
            Some(p) => format!("zone '{p}'"),
            None => "the fleet's zone".to_string(),
        }
    }

    /// The Key Registry key holding this Zone's apex domain.
    pub fn domain_key(&self) -> String {
        self.prefixed(DOMAIN_SUFFIX)
    }

    /// The Key Registry key holding this Zone's Cloudflare API token.
    pub fn token_key(&self) -> String {
        self.prefixed(TOKEN_SUFFIX)
    }

    /// The `Environment=` name caddy reads this Zone's token under. Derived
    /// from [`Zone::token_key`] rather than spelled separately, so the fleet's
    /// keeps the `CLOUDFLARE_DNS_API_TOKEN` the drop-in already writes and a
    /// new Zone cannot be given a name that agrees with nothing (ADR-0082).
    pub fn env_var(&self) -> String {
        self.token_key().to_uppercase()
    }

    fn prefixed(&self, suffix: &str) -> String {
        match &self.prefix {
            Some(p) => format!("{p}_{suffix}"),
            None => suffix.to_string(),
        }
    }
}

/// Both of a Zone's answers for one Host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZonePair {
    pub domain: String,
    pub token: String,
}

/// The Zone `meta` pins its App to, reading both live spellings: `zone:`, the
/// prefix, and ADR-0071's `domain_key:`, the domain key itself.
///
/// A `domain_key:` naming anything but `<prefix>_domain` is refused rather
/// than guessed at: the pair is the Zone, and a key with no derivable sibling
/// names half of one.
pub fn pinned(meta: &PlaybookMeta) -> Result<Option<Zone>> {
    if let Some(prefix) = meta
        .zone
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        return Ok(Some(Zone::named(prefix)));
    }
    let Some(key) = meta
        .domain_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    else {
        return Ok(None);
    };
    if key == DOMAIN_SUFFIX {
        return Ok(Some(Zone::fleet()));
    }
    let Some(prefix) = key.strip_suffix(&format!("_{DOMAIN_SUFFIX}")) else {
        eyre::bail!(
            "domain_key '{key}' names no Zone: a Zone is the pair '<prefix>_{DOMAIN_SUFFIX}' and \
             '<prefix>_{TOKEN_SUFFIX}', so a key outside that shape has no token to go with it"
        );
    };
    Ok(Some(Zone::named(prefix)))
}

/// The Zone `app` is actually in on `host`: the Meta's pin, else the
/// operator's `<app>_zone`, else the fleet's.
///
/// A Meta pin **refuses** a Config answer instead of outranking it silently.
/// The pin is the repo asserting an isolation invariant on every operator's
/// behalf, and an override that is accepted-then-ignored leaves the operator
/// believing the App moved.
pub fn effective_zone(
    meta: &PlaybookMeta,
    config: &Config,
    app: &str,
    host: Option<&str>,
) -> Result<Zone> {
    let declared = config
        .get_for_host(&zone_key(app), host)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    match (pinned(meta)?, declared) {
        (Some(pin), Some(override_)) => eyre::bail!(
            "{app} is pinned to {} by its playbook meta, so '{}' in config.toml (answering \
             '{override_}') cannot move it; the pin states an isolation the repo asserts for \
             every operator (ADR-0081) — drop the config key",
            pin.label(),
            zone_key(app),
        ),
        (Some(pin), None) => Ok(pin),
        (None, Some(prefix)) => Ok(Zone::named(prefix)),
        (None, None) => Ok(Zone::fleet()),
    }
}

/// A Zone's two answers for one Host, or an error naming the half that is
/// missing.
pub fn resolve(zone: &Zone, config: &Config, host: Option<&str>) -> Result<ZonePair> {
    let answer = |key: &str| {
        config
            .get_for_host(key, host)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let where_ = host.map(|h| format!(" for host '{h}'")).unwrap_or_default();

    let (domain_key, token_key) = (zone.domain_key(), zone.token_key());
    let (Some(domain), Some(token)) = (answer(&domain_key), answer(&token_key)) else {
        let missing: Vec<&str> = [domain_key.as_str(), token_key.as_str()]
            .into_iter()
            .filter(|k| answer(k).is_none())
            .collect();
        eyre::bail!(
            "{} is unanswered{where_}: config.toml must answer {} — a Zone is a pair, and a \
             domain without its zone-scoped token is a vhost that can never complete an ACME \
             challenge (ADR-0081)",
            zone.label(),
            missing.join(" and "),
        );
    };
    Ok(ZonePair { domain, token })
}

/// Whether `app` publishes a name at all, and so has a Zone.
///
/// The same question [`crate::services::dns::discover_all_subdomains`] asks
/// per App: the Meta's `subdomain:` default, or the operator's
/// `<app>_subdomain`. An App with no name — a bot, a runtime, a Composition —
/// serves no vhost and publishes no record, so demanding a Zone of it would
/// put a Zone's token on a Host that needs none.
pub fn publishes_a_name(
    meta: &PlaybookMeta,
    config: &Config,
    app: &str,
    host: Option<&str>,
) -> bool {
    let answered = |v: Option<String>| v.map(|s| !s.trim().is_empty()).unwrap_or(false);
    answered(meta.subdomain.clone())
        || answered(config.get_for_host(&format!("{app}_subdomain"), host))
}

/// Every Zone answered for one Host — what its Caddy needs ACME tokens for
/// (ADR-0082).
///
/// Derived, not declared: a Zone is in the set when some App that publishes a
/// name resolves to it *and* its pair answers for this Host. A Host that
/// withdrew the fleet's token contributes no fleet Zone, which is how the
/// agent tier's box ends up holding one token rather than two.
pub fn host_zone_set(
    config: &Config,
    host: &str,
    metas: &[(String, PlaybookMeta)],
) -> Result<BTreeSet<Zone>> {
    let mut zones = BTreeSet::new();
    for (app, meta) in metas {
        if !publishes_a_name(meta, config, app, Some(host)) {
            continue;
        }
        let zone = effective_zone(meta, config, app, Some(host))?;
        if resolve(&zone, config, Some(host)).is_ok() {
            zones.insert(zone);
        }
    }
    Ok(zones)
}

/// Refuse a fleet-wide `<app>_zone`.
///
/// Every other key is fleet-wide with `[hosts.<name>]` as the override; this
/// one inverts that, and the reason is invisible where it is written. A Host's
/// Zone set is what its Caddy holds tokens for, so an un-scoped answer writes
/// a Zone's token onto every Host — including the one ADR-0068 exists to keep
/// the parent domain's token off (ADR-0081).
pub fn assert_no_fleet_wide_zone(config: &Config) -> Result<()> {
    let mut offenders: Vec<String> = config
        .keys()
        .into_iter()
        .filter(|k| k.ends_with(ZONE_SUFFIX))
        .collect();
    if offenders.is_empty() {
        return Ok(());
    }
    offenders.sort();
    eyre::bail!(
        "config.toml answers {} at the top level; an app's zone must be scoped to one host, \
         under [hosts.<name>], because a host's zone set decides which ACME tokens land on it \
         (ADR-0081)",
        offenders.join(", "),
    )
}

fn zone_key(app: &str) -> String {
    format!("{app}{ZONE_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(yaml: &str) -> PlaybookMeta {
        serde_yaml::from_str(yaml).unwrap()
    }

    const BARE: &str = "required_keys: []\nsubdomain: git\n";

    /// Both Zone pairs answered fleet-wide: placing an App in a Zone is the
    /// host-scoped decision, not minting the Zone.
    const ZONES_ANSWERED: &str = r#"
        domain = "fleet.example"
        cloudflare_dns_api_token = "fleet-token"
        studio_domain = "studio.example"
        studio_cloudflare_dns_api_token = "studio-token"
    "#;

    fn config(toml: &str) -> Config {
        Config::from_toml_str(toml).unwrap()
    }

    // ── the pair ──────────────────────────────────────────────────────────────

    #[test]
    fn test_the_fleet_zones_pair_is_unprefixed() {
        let fleet = Zone::fleet();
        assert_eq!(fleet.domain_key(), "domain");
        assert_eq!(fleet.token_key(), "cloudflare_dns_api_token");
        assert_eq!(fleet.env_var(), "CLOUDFLARE_DNS_API_TOKEN");
    }

    #[test]
    fn test_a_named_zones_pair_shares_its_prefix() {
        let studio = Zone::named("studio");
        assert_eq!(studio.domain_key(), "studio_domain");
        assert_eq!(studio.token_key(), "studio_cloudflare_dns_api_token");
        assert_eq!(studio.env_var(), "STUDIO_CLOUDFLARE_DNS_API_TOKEN");
    }

    // ── effective zone ────────────────────────────────────────────────────────

    #[test]
    fn test_no_pin_and_no_config_is_the_fleet_zone() {
        let zone = effective_zone(&meta(BARE), &config(ZONES_ANSWERED), "forgejo", None).unwrap();
        assert_eq!(zone, Zone::fleet());
    }

    #[test]
    fn test_a_host_scoped_app_zone_moves_that_host_only() {
        let config = config(&format!(
            "{ZONES_ANSWERED}\n[hosts.auberge]\nforgejo_zone = \"studio\"\n"
        ));
        assert_eq!(
            effective_zone(&meta(BARE), &config, "forgejo", Some("auberge")).unwrap(),
            Zone::named("studio")
        );
        assert_eq!(
            effective_zone(&meta(BARE), &config, "forgejo", Some("ruche")).unwrap(),
            Zone::fleet()
        );
    }

    #[test]
    fn test_a_meta_pin_places_the_app() {
        let pinned = meta("required_keys: []\nsubdomain: essaim\nzone: agents\n");
        let zone = effective_zone(&pinned, &config(ZONES_ANSWERED), "aoe", None).unwrap();
        assert_eq!(zone, Zone::named("agents"));
    }

    /// ADR-0071's spelling of the same pin, still live. A resolver blind to it
    /// puts the agent tier back in the fleet's Zone.
    #[test]
    fn test_a_domain_key_pin_is_read_as_its_zone() {
        let old = meta("required_keys: []\nsubdomain: essaim\ndomain_key: agents_domain\n");
        assert_eq!(pinned(&old).unwrap(), Some(Zone::named("agents")));
    }

    #[test]
    fn test_a_domain_key_naming_the_fleet_key_is_the_fleet_zone() {
        let old = meta("required_keys: []\nsubdomain: git\ndomain_key: domain\n");
        assert_eq!(pinned(&old).unwrap(), Some(Zone::fleet()));
    }

    #[test]
    fn test_a_domain_key_outside_the_pair_shape_is_refused() {
        let broken = meta("required_keys: []\nsubdomain: git\ndomain_key: studio_apex\n");
        let err = pinned(&broken).unwrap_err().to_string();
        assert!(err.contains("studio_apex"), "{err}");
        assert!(err.contains("pair"), "{err}");
    }

    #[test]
    fn test_a_meta_pin_refuses_a_config_override() {
        let pinned = meta("required_keys: []\nsubdomain: essaim\nzone: agents\n");
        let config = config(&format!(
            "{ZONES_ANSWERED}\n[hosts.ruche]\naoe_zone = \"studio\"\n"
        ));
        let err = effective_zone(&pinned, &config, "aoe", Some("ruche"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("zone 'agents'"), "names the pin: {err}");
        assert!(err.contains("aoe_zone"), "names the config key: {err}");
    }

    // ── resolution ────────────────────────────────────────────────────────────

    #[test]
    fn test_resolving_a_zone_answers_both_halves() {
        let pair = resolve(&Zone::named("studio"), &config(ZONES_ANSWERED), None).unwrap();
        assert_eq!(pair.domain, "studio.example");
        assert_eq!(pair.token, "studio-token");
    }

    #[test]
    fn test_a_zone_with_a_domain_and_no_token_is_unresolved() {
        let config = config(
            r#"
            domain = "fleet.example"
            cloudflare_dns_api_token = "fleet-token"
            studio_domain = "studio.example"
            studio_cloudflare_dns_api_token = ""
        "#,
        );
        let err = resolve(&Zone::named("studio"), &config, Some("auberge"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("studio_cloudflare_dns_api_token"),
            "names the missing half: {err}"
        );
        assert!(
            !err.contains("studio_domain"),
            "not the answered half: {err}"
        );
        assert!(err.contains("auberge"), "names the host: {err}");
    }

    // ── host zone set ─────────────────────────────────────────────────────────

    #[test]
    fn test_one_off_zone_app_gives_its_host_two_zones_and_the_rest_one() {
        let config = config(&format!(
            "{ZONES_ANSWERED}\n[hosts.auberge]\nforgejo_zone = \"studio\"\n"
        ));
        let metas = vec![
            ("forgejo".to_string(), meta(BARE)),
            (
                "navidrome".to_string(),
                meta("required_keys: []\nsubdomain: navidrome\n"),
            ),
        ];
        assert_eq!(
            host_zone_set(&config, "auberge", &metas).unwrap(),
            BTreeSet::from([Zone::fleet(), Zone::named("studio")])
        );
        assert_eq!(
            host_zone_set(&config, "lechuck", &metas).unwrap(),
            BTreeSet::from([Zone::fleet()])
        );
    }

    /// A Host that withdrew the fleet's token holds no fleet Zone — ADR-0068's
    /// isolation, read off the same derivation the drop-in is written from.
    #[test]
    fn test_a_host_withdrawing_a_pair_drops_that_zone_from_its_set() {
        let config = config(
            r#"
            domain = "fleet.example"
            cloudflare_dns_api_token = "fleet-token"
            agents_domain = "agents.example"
            agents_cloudflare_dns_api_token = "agents-token"

            [hosts.ruche]
            cloudflare_dns_api_token = ""
        "#,
        );
        let metas = vec![
            (
                "aoe".to_string(),
                meta("required_keys: []\nsubdomain: essaim\nzone: agents\n"),
            ),
            (
                "navidrome".to_string(),
                meta("required_keys: []\nsubdomain: navidrome\n"),
            ),
        ];
        assert_eq!(
            host_zone_set(&config, "ruche", &metas).unwrap(),
            BTreeSet::from([Zone::named("agents")])
        );
    }

    /// An App with no name has no vhost and no record, so it contributes no
    /// Zone — and cannot drag a token onto a Host that serves nothing.
    #[test]
    fn test_an_app_publishing_no_name_contributes_no_zone() {
        let config = config(ZONES_ANSWERED);
        let metas = vec![("tgtg".to_string(), meta("required_keys: []\n"))];
        assert!(
            host_zone_set(&config, "auberge", &metas)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_an_operator_answered_subdomain_is_a_published_name() {
        let config = config("calibre_subdomain = \"books\"\n");
        assert!(publishes_a_name(
            &meta("required_keys: []\n"),
            &config,
            "calibre",
            None
        ));
    }

    // ── host scope ────────────────────────────────────────────────────────────

    #[test]
    fn test_a_fleet_wide_app_zone_is_refused() {
        let err = assert_no_fleet_wide_zone(&config("forgejo_zone = \"studio\"\n"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("forgejo_zone"), "{err}");
        assert!(err.contains("[hosts."), "names where it belongs: {err}");
    }

    #[test]
    fn test_a_host_scoped_app_zone_is_accepted() {
        let config = config("[hosts.auberge]\nforgejo_zone = \"studio\"\n");
        assert!(assert_no_fleet_wide_zone(&config).is_ok());
    }
}
