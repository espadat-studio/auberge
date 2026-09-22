//! Every vhost states the ACME token its App's **Zone** answers with.
//!
//! Caddy takes one DNS-01 token per process, chosen per Host (ADR-0072). That
//! was the whole problem while each Zone lived on its own Host, and stops
//! being it the moment one Caddy terminates two Zones — a studio forge beside
//! the personal fleet. So the choice moves into the site: every Caddyfile
//! carries `tls { dns cloudflare {env.<name>} }`, naming its own Zone's token
//! (ADR-0082).
//!
//! The name arrives as the App's own Computed Var, never as a literal. An
//! operator moves an App between Zones from `config.toml`, so a spelling
//! written into the template could only ever be the Zone the repo guessed.
//! The token itself cannot be written there at all: `/etc/caddy/sites/` is
//! mode `0644`, while the systemd drop-in that answers `{env.…}` is `0600`.
//!
//! All of them, not only the App that moved. "Absent means inherit the
//! process default" is a hole this file could not see through: the assertion
//! would then only be checkable against the vhosts that opted in, which is
//! the set that was already right.
//!
//! The fleet's line in the drop-in is the one exception, and the reason is
//! ADR-0068. Every App answers its own `<app>_subdomain` fleet-wide, so the
//! fleet Zone is in every Host's Zone set — the agent tier's Host included,
//! and that box must never hold the parent domain's token. Its value is
//! chosen per Host by `infrastructure.yml` (`caddy_acme_token.rs` holds that
//! choice); the Zone set supplies the *named* Zones only. The last test here
//! is what refuses a drop-in that starts taking the fleet's token from the
//! set.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use auberge::services::zone::{DNS_TOKEN_ENV_SUFFIX, HOST_ZONES_VAR, Zone};
use minijinja::value::Value as JValue;
use minijinja::{Environment, UndefinedBehavior};

mod common;

use common::caddy::{Vhost, vhosts};
use common::role_dir;

/// The role variable holding the token for the fleet's Zone, chosen per Host.
const FLEET_INDIRECTION: &str = "caddy_dns_api_token";

/// Written-out tokens, so a branch that resolves to the wrong one has to move
/// an assertion too (ADR-0046).
const FLEET_TOKEN: &str = "parent-zone-token";
const AGENT_TOKEN: &str = "agent-zone-token";
const STUDIO_TOKEN: &str = "studio-zone-token";

/// A vhost carrying no `tls` block, and why it answers no challenge.
///
/// Written out rather than skipped by a rule: a rule would also excuse the
/// next one. A row whose file grew a `tls` block, or lost its site, fails
/// too — the exemption has to keep being true.
const DECLARED_NO_CHALLENGE: &[(&str, &str, &str)] = &[(
    "colporteur",
    "Caddyfile-internal.j2",
    "an `http://localhost:<port>` site, serving the feed files FreshRSS polls over \
     loopback: no public name, no certificate, and so no challenge to answer",
)];

/// Every whole identifier inside a `{{ … }}` on `line`. Whole, because
/// `<app>_dns_api_token_env` contains `<app>_dns_api_token`, and the
/// difference between them is a name and a secret.
///
/// Line-scoped, which is why it is not `roles_compose_off_the_zone.rs`'s
/// file-scoped walk of the same shape: the assertion here is about what one
/// directive names, and a file-wide set would let a vhost pass on a variable
/// mentioned in a log block three lines down.
fn expression_identifiers(line: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = line;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let mut current = String::new();
        for ch in after[..end].chars() {
            if ch.is_alphanumeric() || ch == '_' {
                current.push(ch);
            } else if !current.is_empty() {
                found.insert(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            found.insert(current);
        }
        rest = &after[end + 2..];
    }
    found
}

fn challenge_line(body: &str) -> Option<&str> {
    body.lines().find(|line| line.contains("dns cloudflare"))
}

// ── The reach ─────────────────────────────────────────────────────────────

/// The domain this fence reads, as counts, because a walk that found one
/// vhost would satisfy every assertion below and check one site. Both drift:
/// a role that starts serving a name moves the first, and colporteur's second
/// site is why they differ.
#[test]
fn test_the_fence_reads_every_vhost_in_the_tree() {
    let found = vhosts();
    let roles: BTreeSet<&str> = found.iter().map(|vhost| vhost.role.as_str()).collect();

    assert_eq!(
        found.len(),
        18,
        "the tree deploys a different number of caddy sites than this fence was \
         written against; check the new one's `tls` block, then update this count: \
         {:?}",
        found.iter().map(Vhost::id).collect::<Vec<_>>()
    );
    assert_eq!(
        roles.len(),
        17,
        "17 roles serve those 18 sites — colporteur serves two: {roles:?}"
    );
}

// ── Every vhost names its Zone's variable ─────────────────────────────────

#[test]
fn test_every_vhost_answers_dns_01_with_its_own_apps_variable() {
    let exempt: BTreeSet<(&str, &str)> = DECLARED_NO_CHALLENGE
        .iter()
        .map(|(role, file, _)| (*role, *file))
        .collect();

    for vhost in vhosts() {
        let (role, name) = (&vhost.role, &vhost.template);
        if exempt.contains(&(role.as_str(), name.as_str())) {
            continue;
        }
        let body = vhost.body();
        let line = challenge_line(&body).unwrap_or_else(|| {
            panic!(
                "{role}/{name} answers no DNS-01 challenge of its own: add \
                 `tls {{ dns cloudflare {{env.{{{{ {role}{DNS_TOKEN_ENV_SUFFIX} }}}}}} }}`, \
                 or a row in DECLARED_NO_CHALLENGE saying why this site needs no \
                 certificate (ADR-0082)"
            )
        });
        assert!(
            line.contains("{env."),
            "{role}/{name} must read its token out of caddy's environment — the site \
             file is mode 0644 and the drop-in holding the token is 0600:\n  {line}"
        );
        assert_eq!(
            expression_identifiers(line),
            BTreeSet::from([format!("{role}{DNS_TOKEN_ENV_SUFFIX}")]),
            "{role}/{name} must name its own App's Computed Var and nothing else; an \
             operator moves an App between Zones from config.toml, so a literal here \
             is the Zone the repo guessed (ADR-0081):\n  {line}"
        );
    }
}

/// The exemption has to keep being true. A site that gained a `tls` block, or
/// a row whose file is gone, both leave a reader with a stale reason.
#[test]
fn test_every_declared_exemption_is_still_a_site_that_answers_nothing() {
    let found = vhosts();
    for (role, file, why) in DECLARED_NO_CHALLENGE {
        let site = found
            .iter()
            .find(|vhost| vhost.role == *role && vhost.template == *file)
            .unwrap_or_else(|| {
                panic!(
                    "DECLARED_NO_CHALLENGE names {role}/{file}, which deploys no site; drop the row"
                )
            });
        assert!(
            challenge_line(&site.body()).is_none(),
            "{role}/{file} answers a challenge now, so its exemption is stale — drop \
             the row: {why}"
        );
    }
}

/// No vhost holds a token. The Computed Var carrying the value is one
/// character-sequence away from the one carrying the name, and the file is
/// world-readable.
#[test]
fn test_no_vhost_reads_the_token_itself() {
    let value_var = DNS_TOKEN_ENV_SUFFIX
        .strip_suffix("_env")
        .expect("the name var is the value var plus `_env`");
    for vhost in vhosts() {
        let (role, name) = (&vhost.role, &vhost.template);
        let leaked: Vec<String> = vhost
            .body()
            .lines()
            .flat_map(expression_identifiers)
            .filter(|id| id.ends_with(value_var) || id == "cloudflare_dns_api_token")
            .collect();
        assert!(
            leaked.is_empty(),
            "{role}/{name} is mode 0644 and reads a token into it: {leaked:?}. Read \
             `{role}{DNS_TOKEN_ENV_SUFFIX}` and let caddy resolve `{{env.…}}` (ADR-0082)"
        );
    }
}

// ── What the drop-in writes ───────────────────────────────────────────────

fn jinja() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_filter(
        "from_json",
        |raw: &str| -> Result<JValue, minijinja::Error> {
            serde_json::from_str::<serde_json::Value>(raw)
                .map(|parsed| JValue::from_serialize(&parsed))
                .map_err(|e| {
                    minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                })
        },
    );
    env
}

/// The drop-in, rendered against the Host's Zone set and the token
/// `infrastructure.yml` chose for the fleet's Zone.
fn render(fleet_token: &str, zones: &[(Option<&str>, &str)]) -> String {
    let template = fs::read_to_string(role_dir("caddy").join("templates/caddy-env.conf.j2"))
        .expect("caddy-env.conf.j2 must exist");
    let set: Vec<serde_json::Value> = zones
        .iter()
        .map(|(prefix, token)| {
            let zone = prefix.map_or_else(Zone::fleet, Zone::named);
            serde_json::json!({
                "prefix": prefix,
                "domain": "zone.example",
                "token": token,
                "env": zone.env_var(),
            })
        })
        .collect();
    let context = BTreeMap::from([
        (FLEET_INDIRECTION, JValue::from(fleet_token)),
        (
            HOST_ZONES_VAR,
            JValue::from(serde_json::to_string(&set).unwrap()),
        ),
    ]);
    jinja()
        .render_str(&template, JValue::from_serialize(&context))
        .expect("caddy-env.conf.j2 must render")
}

/// Every `NAME=value` the rendered drop-in sets, in order.
fn environment(rendered: &str) -> Vec<(String, String)> {
    rendered
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Environment="))
        .map(|assignment| {
            let unquoted = assignment.trim_matches('"');
            let (name, value) = unquoted
                .split_once('=')
                .unwrap_or_else(|| panic!("`Environment={assignment}` sets nothing"));
            assert!(
                assignment.starts_with('"') && assignment.ends_with('"'),
                "`{assignment}` must be quoted, or systemd splits the value on whitespace"
            );
            (name.to_string(), value.to_string())
        })
        .collect()
}

/// A Host serving one Zone holds one token, under the name it has held since
/// ADR-0072 — the name every fleet vhost resolves to, on every Host already.
#[test]
fn test_a_host_serving_only_the_fleet_writes_one_line() {
    assert_eq!(
        environment(&render(FLEET_TOKEN, &[(None, FLEET_TOKEN)])),
        vec![(Zone::fleet().env_var(), FLEET_TOKEN.to_string())]
    );
}

/// A Host with nothing to say still gets the fleet's line: the caddy role runs
/// on every Host, including one publishing no name at all.
#[test]
fn test_a_host_with_an_empty_zone_set_still_gets_the_fleets_line() {
    assert_eq!(
        environment(&render(FLEET_TOKEN, &[])),
        vec![(Zone::fleet().env_var(), FLEET_TOKEN.to_string())]
    );
}

/// The point of the change: two Zones on one Caddy, each vhost resolving its
/// own.
#[test]
fn test_a_host_serving_a_second_zone_writes_both_tokens() {
    let rendered = render(
        FLEET_TOKEN,
        &[(None, FLEET_TOKEN), (Some("studio"), STUDIO_TOKEN)],
    );
    assert_eq!(
        environment(&rendered),
        vec![
            (Zone::fleet().env_var(), FLEET_TOKEN.to_string()),
            (Zone::named("studio").env_var(), STUDIO_TOKEN.to_string()),
        ],
        "{rendered}"
    );
}

/// ADR-0068, held where this change could have broken it.
///
/// The agent tier's Host resolves the fleet Zone like every other — its Apps
/// answer their subdomains fleet-wide — so the Zone set hands the drop-in the
/// parent domain's token. Taking the fleet's line from that set would write
/// it onto the one box ADR-0054 assumes compromisable.
#[test]
fn test_the_parent_domains_token_never_reaches_the_agent_tiers_host() {
    let rendered = render(
        AGENT_TOKEN,
        &[(None, FLEET_TOKEN), (Some("agents"), AGENT_TOKEN)],
    );
    assert!(
        !rendered.contains(FLEET_TOKEN),
        "the parent domain's token must not be written onto a Host serving the agent \
         tier (ADR-0068); the fleet's line takes `{FLEET_INDIRECTION}`, never the Zone \
         set's own token:\n{rendered}"
    );
}

/// And the fleet's *name* does not survive there either.
///
/// `infrastructure.yml` hands that Host the agent tier's token, so writing it
/// under the fleet's name leaves a name that lies: a fleet-Zone vhost on that
/// Host would answer DNS-01 with a token scoped to another zone, deploy
/// green, and fail weeks later at renewal. Twelve vhosts answered over
/// HTTP-01 before this change and did not care which token was there; they do
/// now. Absent, caddy refuses to start and the Ingress Gate reports it in the
/// same run.
#[test]
fn test_a_host_answering_for_a_named_zone_holds_that_zones_token_alone() {
    let rendered = render(
        AGENT_TOKEN,
        &[(None, FLEET_TOKEN), (Some("agents"), AGENT_TOKEN)],
    );
    assert_eq!(
        environment(&rendered),
        vec![(Zone::named("agents").env_var(), AGENT_TOKEN.to_string())],
        "{rendered}"
    );
}
