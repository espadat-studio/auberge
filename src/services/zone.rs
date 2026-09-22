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

use crate::config::Config;
use crate::playbook_meta::PlaybookMeta;
use eyre::{Result, WrapErr};
use serde::Serialize;
use std::collections::BTreeMap;

/// The second half of a Zone's key pair, unprefixed.
const DOMAIN_SUFFIX: &str = "domain";

/// The first half of a Zone's key pair, unprefixed.
const TOKEN_SUFFIX: &str = "cloudflare_dns_api_token";

/// The Config key placing an App in a Zone: `<app>_zone`.
const ZONE_SUFFIX: &str = "_zone";

/// The Config key answering an App's serving gate: `<app>_subdomain`.
const SUBDOMAIN_SUFFIX: &str = "_subdomain";

/// The Computed Var holding an App's resolved apex: `<app>_parent_domain`.
pub const PARENT_DOMAIN_SUFFIX: &str = "_parent_domain";

/// The Computed Var holding an App's resolved Cloudflare token:
/// `<app>_dns_api_token`. Named for what a role does with it rather than for
/// the Registry key it came out of — a role composing a `dns_record` call
/// asks its App for a token, never a Zone for one.
pub const DNS_TOKEN_SUFFIX: &str = "_dns_api_token";

/// The Computed Var holding the name of the environment variable an App's
/// vhost reads its Zone's token from: `<app>_dns_api_token_env`.
///
/// The name, not the token. `/etc/caddy/sites/*.caddyfile` is mode 0644, so a
/// vhost states `{env.<name>}` and caddy resolves it out of a 0600 drop-in
/// (ADR-0082). A literal in the template could only spell the Zone the repo
/// guessed, which is the one thing an operator-placed App moves.
pub const DNS_TOKEN_ENV_SUFFIX: &str = "_dns_api_token_env";

/// The Computed Var holding every Zone one Host serves, as JSON: what its
/// Caddy needs an ACME token for (ADR-0082).
///
/// **Its value embeds Cloudflare tokens under a name that `config.rs`'s
/// `SENSITIVE_SUFFIXES` test does not match.** Every other secret a run
/// carries is named `…_token` or `…_key` and is redacted by that suffix
/// test; this one is a list, and no honest name for a list of Zones carries
/// the suffix. Nothing renders a run's variables today, so there is no leak
/// path — but a caller that ever does must redact this name explicitly
/// rather than by heuristic, and the Ansible task reading it needs `no_log`.
pub const HOST_ZONES_VAR: &str = "host_zones";

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

    /// The prefix its key pair shares, `None` for the fleet's Zone. The same
    /// shape `<app>_zone` is answered in, so a caller can hand it straight
    /// back to an operator.
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
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

    /// The `Environment=` name this Zone's token lands under in caddy's
    /// systemd drop-in, and the name every vhost in the Zone reads it back
    /// with — `{env.<this>}` (ADR-0082).
    ///
    /// The Zone's token key, uppercased. One rule rather than a spelling of
    /// its own, and it is what makes the fleet's name come out as
    /// `CLOUDFLARE_DNS_API_TOKEN` — the name caddy has read since ADR-0072,
    /// which is on every Host already and cannot move without every vhost
    /// naming a variable no drop-in writes.
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

/// One Zone a Host serves, as [`HOST_ZONES_VAR`] hands it to Ansible.
///
/// Carries the token beside the prefix rather than leaving the template to
/// look one up by name: a Zone is a pair (ADR-0081), and an entry naming a
/// Zone whose token the template resolves separately is the same pair split
/// across two expressions again.
///
/// The prefix is `null` for the fleet's Zone, which is the shape
/// [`Zone::prefix`] answers in — a template deriving a name from it decides
/// how the fleet's Zone spells that name, and nothing here pre-empts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct HostZone {
    pub prefix: Option<String>,
    pub domain: String,
    pub token: String,
    pub env: String,
}

/// The Zone `meta` pins its App to: its `zone:` prefix, or nothing.
///
/// A prefix is a Zone's whole identity, so there is no shape to reject here
/// and no way for this to fail. A prefix naming no Zone is still possible and
/// is caught where the answer is: `tests/zone_declaration.rs` refuses a pin
/// whose pair the Key Registry does not hold, and [`resolve`] refuses one the
/// operator has not answered for the Host.
pub fn pinned(meta: &PlaybookMeta) -> Option<Zone> {
    meta.zone
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(Zone::named)
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

    match (pinned(meta), declared) {
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

/// The Zone `app`'s published record lives in, asked without a Host.
///
/// [`effective_zone`] answers "which Zone on host X". `dns` cannot ask that:
/// it holds one Zone per run and targets no Host, while `<app>_zone` is
/// host-scoped. So it asks the question it can act on — is this App's record
/// outside the Zone this run holds, anywhere — and a placement under any
/// `[hosts.<name>]` answers yes. A record in another Zone is out of a
/// fleet-Zone run's reach whichever Host serves it.
///
/// A Meta pin wins, as it does everywhere. The pin-plus-override refusal
/// lives in Preflight, not here: this is a read, and classifying by the pin
/// is the right classification while the operator is told at deploy time.
/// `config` is optional because the callers walk the tree before a Config is
/// guaranteed to load; without one only the Meta can place an App.
pub fn publication_zone(config: Option<&Config>, app: &str, meta: &PlaybookMeta) -> Zone {
    if let Some(pin) = pinned(meta) {
        return pin;
    }
    let Some(config) = config else {
        return Zone::fleet();
    };
    let key = zone_key(app);
    let scopes = std::iter::once(None).chain(config.host_override_names().into_iter().map(Some));
    for scope in scopes {
        if let Some(prefix) = config
            .get_for_host(&key, scope.as_deref())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            return Zone::named(prefix);
        }
    }
    Zone::fleet()
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
/// `<app>_subdomain`. Two expressions of one rule, which is the divergence
/// class ADR-0081 exists to delete — they converge when `services::dns`
/// delegates here, and until then a name this says is published and that one
/// does not is a Zone demanded for a record nothing writes.
/// An App with no name — a bot, a runtime, a Composition —
/// serves no vhost and publishes no record, so demanding a Zone of it would
/// put a Zone's token on a Host that needs none.
pub fn publishes_a_name(
    meta: &PlaybookMeta,
    config: &Config,
    app: &str,
    host: Option<&str>,
) -> bool {
    let named = meta
        .subdomain
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    named || serving_gate_answered(config, app, host)
}

/// Whether the operator answered `app`'s **serving gate** — `<app>_subdomain`
/// — for `host`.
///
/// The config half of [`publishes_a_name`], asked alone, and the only half a
/// caller can use to decide whether a `when:`-guarded role *runs* here.
///
/// The two halves answer different questions. A Meta's `subdomain:` is the
/// name an App publishes **once it runs** — true of every Host, since it is a
/// repo fact. The Config answer is the operator turning the App on for one
/// Host: config alone answers it and a blank value is no answer (ADR-0051),
/// scoped under `[hosts.<name>]` like every other one (ADR-0058). A guarded
/// role's Zone therefore follows this half and never the Meta's, which is
/// exactly the half that cannot tell the two apart (ADR-0083).
pub fn serving_gate_answered(config: &Config, app: &str, host: Option<&str>) -> bool {
    crate::hosts::gate_answered(config, &format!("{app}{SUBDOMAIN_SUFFIX}"), host)
}

/// Every App that publishes a name on `host`, the Zone it lands in and that
/// Zone's answers — the one walk both derivations above and below read.
///
/// "Which name does this App compose against" and "which tokens does this
/// Host hold" are the same walk read two ways. Deriving them separately is
/// two predicates a test hopes agree, which is the divergence class ADR-0081
/// deletes rather than fences.
///
/// A Zone that does not answer for this Host drops its App silently: an
/// operator who never onboarded a second Zone has no name there to publish,
/// and the run that would have published one is refused in Preflight, where
/// the App and the missing key can both be named.
fn placements<'a>(
    config: &Config,
    host: &str,
    metas: &'a [(String, PlaybookMeta)],
) -> Result<Vec<(&'a str, Zone, ZonePair)>> {
    let mut placed = Vec::new();
    for (app, meta) in metas {
        if !publishes_a_name(meta, config, app, Some(host)) {
            continue;
        }
        let zone = effective_zone(meta, config, app, Some(host))?;
        let Ok(pair) = resolve(&zone, config, Some(host)) else {
            continue;
        };
        placed.push((app.as_str(), zone, pair));
    }
    Ok(placed)
}

/// Every **Computed Var** one run hands Ansible: each App's resolved Zone,
/// and the Zone set of the Host it runs against (ADR-0081, ADR-0082).
///
/// A Computed Var is not an Injected Key. An Injected Key is in the Key
/// Registry precisely so a stale `config.toml` value can be overridden
/// (ADR-0063); a resolved Zone has no such value to override, so these names
/// are absent from the Registry and `config set` will not offer one. The
/// overlay onto a run's variables is written *after* config, so a
/// hand-written entry of one of these names reaches no role.
///
/// Computed for every App with a Meta, not only the run's: blocky builds its
/// `customDNS` map `run_once` over all of them, so a map missing the Apps
/// this run happens not to deploy is a name the tailnet stops resolving.
///
/// The Host's Zone set is derived, not declared: a Zone is in it when some
/// App that publishes a name resolves to it *and* its pair answers for this
/// Host. A Host that withdrew a Zone's token contributes no such Zone, which
/// is how the agent tier's box ends up holding one token rather than two.
pub fn computed_vars(
    metas: &[(String, PlaybookMeta)],
    config: &Config,
    host: &str,
) -> Result<BTreeMap<String, String>> {
    let placed = placements(config, host, metas)?;

    let mut vars = BTreeMap::new();
    for (app, zone, pair) in &placed {
        vars.insert(format!("{app}{PARENT_DOMAIN_SUFFIX}"), pair.domain.clone());
        vars.insert(format!("{app}{DNS_TOKEN_SUFFIX}"), pair.token.clone());
        vars.insert(format!("{app}{DNS_TOKEN_ENV_SUFFIX}"), zone.env_var());
    }

    let zones: BTreeMap<&Zone, &ZonePair> =
        placed.iter().map(|(_, zone, pair)| (zone, pair)).collect();
    let set: Vec<HostZone> = zones
        .into_iter()
        .map(|(zone, pair)| HostZone {
            prefix: zone.prefix().map(str::to_string),
            domain: pair.domain.clone(),
            token: pair.token.clone(),
            env: zone.env_var(),
        })
        .collect();
    vars.insert(
        HOST_ZONES_VAR.to_string(),
        serde_json::to_string(&set).wrap_err("Failed to serialize the host's zone set")?,
    );

    Ok(vars)
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
    }

    #[test]
    fn test_a_named_zones_pair_shares_its_prefix() {
        let studio = Zone::named("studio");
        assert_eq!(studio.domain_key(), "studio_domain");
        assert_eq!(studio.token_key(), "studio_cloudflare_dns_api_token");
    }

    /// The fleet's spelling is fixed by what is already on every Host: caddy
    /// has read `CLOUDFLARE_DNS_API_TOKEN` since ADR-0072, and a Zone that
    /// renamed it would leave every vhost naming a variable no drop-in writes.
    #[test]
    fn test_the_fleet_zones_token_lands_under_the_cloudflare_env_var() {
        assert_eq!(Zone::fleet().env_var(), "CLOUDFLARE_DNS_API_TOKEN");
    }

    #[test]
    fn test_a_named_zones_env_var_carries_its_prefix() {
        assert_eq!(
            Zone::named("studio").env_var(),
            "STUDIO_CLOUDFLARE_DNS_API_TOKEN"
        );
        assert_eq!(
            Zone::named("agents").env_var(),
            "AGENTS_CLOUDFLARE_DNS_API_TOKEN"
        );
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
    //
    // Read off `computed_vars`, which is the one derivation: the set the
    // drop-in is written from and the pair each App composes against come out
    // of the same walk, so neither can be narrowed without the other moving.

    /// A Host that withdrew a Zone's token holds no such Zone — ADR-0068's
    /// isolation, arrived at from the derivation rather than declared.
    #[test]
    fn test_a_host_withdrawing_a_pair_drops_that_zone_from_its_set() {
        let toml = r#"
            domain = "fleet.example"
            cloudflare_dns_api_token = "fleet-token"
            agents_domain = "agents.example"
            agents_cloudflare_dns_api_token = "agents-token"

            [hosts.ruche]
            cloudflare_dns_api_token = ""
        "#;
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
        let vars = computed(toml, "ruche", &metas);
        assert_eq!(
            zone_set(&vars)
                .into_iter()
                .map(|z| z.prefix)
                .collect::<Vec<_>>(),
            vec![Some("agents".to_string())]
        );
        assert!(
            !vars.contains_key("navidrome_parent_domain"),
            "the withdrawn Zone must take its Apps with it: {vars:?}"
        );
    }

    /// An App with no name has no vhost and no record, so it contributes no
    /// Zone — and cannot drag a token onto a Host that serves nothing.
    #[test]
    fn test_an_app_publishing_no_name_contributes_no_zone() {
        let metas = vec![("tgtg".to_string(), meta("required_keys: []\n"))];
        assert_eq!(
            zone_set(&computed(ZONES_ANSWERED, "auberge", &metas)),
            vec![]
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

    // ── computed vars ─────────────────────────────────────────────────────────

    fn computed(
        toml: &str,
        host: &str,
        metas: &[(String, PlaybookMeta)],
    ) -> BTreeMap<String, String> {
        computed_vars(metas, &config(toml), host).unwrap()
    }

    fn zone_set(vars: &BTreeMap<String, String>) -> Vec<HostZone> {
        serde_json::from_str(&vars[HOST_ZONES_VAR]).expect("the host zone set must be JSON")
    }

    /// Both halves land under the App's own name, so a role asks its App and
    /// never a Zone. Written-out values, not the keys read back, or the
    /// assertion holds for a resolver that returns its own input.
    #[test]
    fn test_an_app_gets_both_halves_of_its_zone() {
        let metas = vec![("navidrome".to_string(), meta(BARE))];
        let vars = computed(ZONES_ANSWERED, "auberge", &metas);
        assert_eq!(vars["navidrome_parent_domain"], "fleet.example");
        assert_eq!(vars["navidrome_dns_api_token"], "fleet-token");
    }

    /// The point of the whole model: an App the operator placed elsewhere
    /// composes against *that* Zone, and does so on that Host alone.
    #[test]
    fn test_an_off_zone_app_gets_its_own_zones_pair() {
        let toml = format!("{ZONES_ANSWERED}\n[hosts.auberge]\nforgejo_zone = \"studio\"\n");
        let metas = vec![("forgejo".to_string(), meta(BARE))];

        let auberge = computed(&toml, "auberge", &metas);
        assert_eq!(auberge["forgejo_parent_domain"], "studio.example");
        assert_eq!(auberge["forgejo_dns_api_token"], "studio-token");

        let ruche = computed(&toml, "ruche", &metas);
        assert_eq!(ruche["forgejo_parent_domain"], "fleet.example");
        assert_eq!(ruche["forgejo_dns_api_token"], "fleet-token");
    }

    /// An App with no name serves no vhost and writes no record, so it is
    /// handed no Zone — a var for one would be a token in a run that reads it
    /// for nothing.
    #[test]
    fn test_an_app_publishing_no_name_gets_no_vars() {
        let metas = vec![("tgtg".to_string(), meta("required_keys: []\n"))];
        let vars = computed(ZONES_ANSWERED, "auberge", &metas);
        assert!(!vars.contains_key("tgtg_parent_domain"), "{vars:?}");
        assert!(!vars.contains_key("tgtg_dns_api_token"), "{vars:?}");
    }

    /// An unanswered Zone yields absence, not an empty string: a role reading
    /// an undefined var fails the play, where one reading `""` composes
    /// `essaim.` and publishes it (ADR-0071).
    #[test]
    fn test_an_unanswered_zone_yields_no_vars_rather_than_empty_ones() {
        let metas = vec![(
            "aoe".to_string(),
            meta("required_keys: []\nsubdomain: essaim\nzone: agents\n"),
        )];
        let vars = computed(ZONES_ANSWERED, "auberge", &metas);
        assert!(!vars.contains_key("aoe_parent_domain"), "{vars:?}");
        assert!(!vars.contains_key("aoe_dns_api_token"), "{vars:?}");
    }

    /// A pin the operator tried to override fails the computation rather than
    /// resolving one of the two answers: the run is about to write a vhost,
    /// and neither answer is the one the operator believes in.
    #[test]
    fn test_a_pin_the_config_contradicts_fails_the_computation() {
        let toml = format!("{ZONES_ANSWERED}\n[hosts.auberge]\naoe_zone = \"studio\"\n");
        let metas = vec![(
            "aoe".to_string(),
            meta("required_keys: []\nsubdomain: essaim\nzone: agents\n"),
        )];
        let err = computed_vars(&metas, &config(&toml), "auberge")
            .unwrap_err()
            .to_string();
        assert!(err.contains("aoe_zone"), "{err}");
    }

    /// The Host's Zone set carries each Zone once with its pair, so the
    /// drop-in writes one line per Zone and reads no token by name.
    #[test]
    fn test_the_host_zone_set_carries_each_zone_once_with_its_pair() {
        let toml = format!("{ZONES_ANSWERED}\n[hosts.auberge]\nforgejo_zone = \"studio\"\n");
        let metas = vec![
            ("forgejo".to_string(), meta(BARE)),
            (
                "navidrome".to_string(),
                meta("required_keys: []\nsubdomain: navidrome\n"),
            ),
            (
                "calibre".to_string(),
                meta("required_keys: []\nsubdomain: books\n"),
            ),
        ];
        assert_eq!(
            zone_set(&computed(&toml, "auberge", &metas)),
            vec![
                HostZone {
                    prefix: None,
                    domain: "fleet.example".to_string(),
                    token: "fleet-token".to_string(),
                    env: "CLOUDFLARE_DNS_API_TOKEN".to_string(),
                },
                HostZone {
                    prefix: Some("studio".to_string()),
                    domain: "studio.example".to_string(),
                    token: "studio-token".to_string(),
                    env: "STUDIO_CLOUDFLARE_DNS_API_TOKEN".to_string(),
                },
            ]
        );
    }

    /// An App's vhost names an environment variable, never a token: the
    /// Caddyfile is mode 0644 (ADR-0082). The name is the App's own Computed
    /// Var so that an App the operator moved names its new Zone's variable
    /// without a repo edit — a literal in the template could only ever spell
    /// the Zone the repo guessed.
    #[test]
    fn test_an_app_is_handed_the_env_var_name_of_its_zone() {
        let toml = format!("{ZONES_ANSWERED}\n[hosts.auberge]\nforgejo_zone = \"studio\"\n");
        let metas = vec![
            ("forgejo".to_string(), meta(BARE)),
            (
                "navidrome".to_string(),
                meta("required_keys: []\nsubdomain: navidrome\n"),
            ),
        ];
        let vars = computed(&toml, "auberge", &metas);
        assert_eq!(
            vars["forgejo_dns_api_token_env"],
            "STUDIO_CLOUDFLARE_DNS_API_TOKEN"
        );
        assert_eq!(
            vars["navidrome_dns_api_token_env"],
            "CLOUDFLARE_DNS_API_TOKEN"
        );
    }

    /// The two sides of one deploy: the name an App's vhost reads is the name
    /// the Host's drop-in writes. They come off one derivation, and this is
    /// the assertion that fails if a later edit spells either separately.
    #[test]
    fn test_every_apps_env_var_is_one_the_hosts_drop_in_writes() {
        let toml = format!("{ZONES_ANSWERED}\n[hosts.auberge]\nforgejo_zone = \"studio\"\n");
        let metas = vec![
            ("forgejo".to_string(), meta(BARE)),
            (
                "navidrome".to_string(),
                meta("required_keys: []\nsubdomain: navidrome\n"),
            ),
        ];
        let vars = computed(&toml, "auberge", &metas);
        let written: Vec<(String, String)> = zone_set(&vars)
            .into_iter()
            .map(|zone| (zone.env, zone.token))
            .collect();
        for app in ["forgejo", "navidrome"] {
            let read = (
                vars[&format!("{app}{DNS_TOKEN_ENV_SUFFIX}")].clone(),
                vars[&format!("{app}{DNS_TOKEN_SUFFIX}")].clone(),
            );
            assert!(
                written.contains(&read),
                "{app} reads {read:?}, absent from {written:?}"
            );
        }
    }

    /// A Host serving one Zone holds one token. The derivation that widens a
    /// Host's token set is the one ADR-0068 exists to bound.
    #[test]
    fn test_a_host_with_no_off_zone_app_holds_one_zone() {
        let toml = format!("{ZONES_ANSWERED}\n[hosts.auberge]\nforgejo_zone = \"studio\"\n");
        let metas = vec![("forgejo".to_string(), meta(BARE))];
        assert_eq!(
            zone_set(&computed(&toml, "lechuck", &metas))
                .into_iter()
                .map(|z| z.prefix)
                .collect::<Vec<_>>(),
            vec![None]
        );
    }

    /// Every App's pair is one the Host's set also holds. Both come off one
    /// walk, and this is the assertion that fails if a later edit splits them.
    #[test]
    fn test_every_apps_pair_is_in_its_hosts_zone_set() {
        let toml = format!("{ZONES_ANSWERED}\n[hosts.auberge]\nforgejo_zone = \"studio\"\n");
        let metas = vec![
            ("forgejo".to_string(), meta(BARE)),
            (
                "navidrome".to_string(),
                meta("required_keys: []\nsubdomain: navidrome\n"),
            ),
        ];
        let vars = computed(&toml, "auberge", &metas);
        let pairs: Vec<(String, String)> = zone_set(&vars)
            .into_iter()
            .map(|z| (z.domain, z.token))
            .collect();
        for app in ["forgejo", "navidrome"] {
            let pair = (
                vars[&format!("{app}{PARENT_DOMAIN_SUFFIX}")].clone(),
                vars[&format!("{app}{DNS_TOKEN_SUFFIX}")].clone(),
            );
            assert!(
                pairs.contains(&pair),
                "{app} resolves to {pair:?}, absent from {pairs:?}"
            );
        }
    }

    /// An empty set is still an answer: the drop-in writer needs the var
    /// defined to iterate it, and a Host serving nothing writes no line.
    #[test]
    fn test_a_host_serving_nothing_still_gets_an_empty_zone_set() {
        let vars = computed(ZONES_ANSWERED, "auberge", &[]);
        assert_eq!(zone_set(&vars), vec![]);
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
