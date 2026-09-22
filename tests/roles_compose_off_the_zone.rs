//! A role composes its App's public name off its App's **Zone**, never off a
//! Key Registry key of its own choosing.
//!
//! Before ADR-0081 there was one zone, so `{{ domain }}` was both "the fleet's
//! apex" and "where this App's name lives", and nothing had to tell them
//! apart. A second zone splits them: `git` moves to the studio's apex while
//! sixteen other names stay on the fleet's, and every expression that spelled
//! the apex directly now has to say *whose* apex it meant.
//!
//! The CLI answers that once per run and hands each App its own pair down as
//! Computed Vars — [`PARENT_DOMAIN_SUFFIX`] and [`DNS_TOKEN_SUFFIX`]. A role
//! reads its App's, and that is the whole protocol. What this fence refuses is
//! the shape that looks identical on a green deploy: a role that resolves a
//! Zone itself. `{{ domain }}` in a composition, `{{ agents_domain }}` in
//! another, `{{ cloudflare_dns_api_token }}` beside a `dns_record` call — each
//! is a second answer to a question the CLI already answered, and a second
//! answer only diverges once somebody moves an App.
//!
//! ## Uniform, not minimal
//!
//! Only `forgejo` moves, and only `aoe` is already elsewhere. Every role reads
//! its Computed Var anyway, because two roles that can change Zone and sixteen
//! that cannot — with nothing in the tree saying which is which — is a
//! distinction the next reader has to rediscover from the rollout notes. `aoe`
//! is in the uniform set for its own reason: it composed off
//! `{{ agents_domain }}`, the same defect spelled with the other zone's key.
//!
//! ## What this replaces
//!
//! `tailnet_only_parent_domain.rs` held the same composition when Blocky
//! resolved it, and one of its assertions was that a Public App may *not*
//! declare a Zone — public-plus-off-Zone being unbuildable then. That premise
//! is what this work makes false, so the file goes. Its surviving properties
//! land here (the Blocky evaluation below) or were already held more strictly
//! elsewhere: `zone_declaration.rs` demands a pin name a Zone whose *pair* the
//! registry holds, where the old file asked only that the domain key exist.
//!
//! ## Reach
//!
//! Three walks, each stated as a count or a written-out set, all drifting with
//! the tree: the 18 roles that define an `<app>_domain`, the 11 `dns_record`
//! call sites, and every remaining direct read of a Zone's registry pair. Each
//! walks the tree, so a walk that quietly stops reaching somewhere passes its
//! assertions over nothing at all.
//!
//! The third exists because the first two are shape-bound, and a role can
//! spell the fleet's apex somewhere neither reaches: `yourls.caddyfile.j2`
//! published `yourls.{fleet}` off its own site line, and blocky asked Lego for
//! a certificate on `blocky_domain` with a token from a Zone it may not be in.
//! Both are fixed here, and reverting either passed the first two walks. So
//! the third is the complement, and a declared regime rather than a drift
//! check: every surviving read is written down with its reason, and an
//! undeclared one is refused.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use minijinja::value::{Kwargs, Value as JValue};
use minijinja::{Environment, State, UndefinedBehavior};
use serde_yaml::Value;

mod common;

use auberge::services::zone::{DNS_TOKEN_SUFFIX, PARENT_DOMAIN_SUFFIX};
use common::apps::app_of;
use common::{
    Task, all_roles, defaults, field, registry_keys, role_dir, role_tasks, role_template_files,
    role_yml_files, task_name,
};

/// The role every `dns_record` call site includes.
const DNS_RECORD_ROLE: &str = "dns_record";

/// The fact Blocky's accumulator builds. `blocky_tailnet_addresses.rs` names
/// the same one, and both fences find the task by it.
const ADDRESS_MAP: &str = "blocky_tailscale_domain_addresses";

/// The two parameters that role takes for *where* to write, and the two
/// Computed Vars that answer them.
const RECORD_DOMAIN_PARAM: &str = "dns_record_domain";
const RECORD_TOKEN_PARAM: &str = "dns_record_cloudflare_api_token";

// ── The compositions the tree declares ────────────────────────────────────

/// Every `<app>_domain` a role defaults, as `(role, expression)`. The role's
/// own name is the App's here, and [`app_of`] confirms it below rather than
/// this walk assuming it.
fn domain_defaults() -> Vec<(String, String)> {
    all_roles()
        .into_iter()
        .filter_map(|role| {
            let expression = defaults(&role).get(&format!("{role}_domain"))?.clone();
            Some((role, expression))
        })
        .collect()
}

/// Every task that includes [`DNS_RECORD_ROLE`], as `(role, task name, vars)`.
fn dns_record_calls() -> Vec<(String, String, BTreeMap<String, String>)> {
    all_roles()
        .into_iter()
        .filter(|role| role != DNS_RECORD_ROLE)
        .flat_map(|role| {
            role_tasks(&role)
                .into_iter()
                .filter(includes_dns_record)
                .map(|task| {
                    let vars = field(&task.body, "vars")
                        .and_then(Value::as_mapping)
                        .map(|mapping| {
                            mapping
                                .iter()
                                .filter_map(|(name, value)| {
                                    Some((name.as_str()?.to_string(), value.as_str()?.to_string()))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    (role.clone(), task_name(&task.body).to_string(), vars)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn includes_dns_record(task: &Task) -> bool {
    field(&task.body, "ansible.builtin.include_role")
        .and_then(Value::as_mapping)
        .and_then(|include| field(include, "name"))
        .and_then(Value::as_str)
        == Some(DNS_RECORD_ROLE)
}

/// The domain this fence reads, as counts. `domain_defaults` and
/// `dns_record_calls` both hold over the empty set, so a walk that stopped
/// reaching the tree would satisfy every assertion below without reading a
/// line of it.
#[test]
fn the_fence_reads_every_composition_and_every_dns_record_call() {
    let compositions = domain_defaults();
    let calls = dns_record_calls();

    assert_eq!(
        compositions.len(),
        18,
        "a role defining an <app>_domain appeared or vanished; check its composition \
         against the assertions below, then update this count: {:?}",
        compositions.iter().map(|(r, _)| r).collect::<Vec<_>>()
    );
    assert_eq!(
        calls.len(),
        11,
        "a dns_record call site appeared or vanished; check the pair it passes, then \
         update this count: {:?}",
        calls.iter().map(|(r, _, _)| r).collect::<Vec<_>>()
    );
}

/// The composition, written out. An App's public name is its subdomain under
/// the apex its Zone resolved to, and nothing else is a name this repo can
/// both publish a record for and answer a DNS-01 challenge on.
///
/// Compared as a literal rather than searched for a substring (ADR-0046): a
/// `contains` would pass over `{{ x_subdomain }}.{{ domain }}.{{
/// x_parent_domain }}`, and more usefully, an edit that moves the shape has to
/// move this line too.
#[test]
fn every_role_composes_its_app_name_off_its_apps_zone() {
    let wrong: Vec<String> = domain_defaults()
        .into_iter()
        .filter(|(role, expression)| {
            *expression
                != format!("{{{{ {role}_subdomain }}}}.{{{{ {role}{PARENT_DOMAIN_SUFFIX} }}}}")
        })
        .map(|(role, expression)| format!("{role}: {expression}"))
        .collect();

    assert!(
        wrong.is_empty(),
        "a role resolves its own parent domain instead of reading the Computed Var the \
         CLI hands it. `<app>_subdomain`.`<app>{PARENT_DOMAIN_SUFFIX}` is the only \
         composition that follows the App when its Zone moves (ADR-0081): {wrong:?}"
    );
}

/// A role's name is the App's name, which is what makes `<role>_parent_domain`
/// resolvable at all — the CLI keys the Computed Vars by the App the Playbook
/// Meta names. A role whose App is spelled differently would compose against a
/// variable no run ever sets, and read as a working default until it deployed.
#[test]
fn every_composing_role_is_named_for_its_app() {
    let mismatched: Vec<String> = domain_defaults()
        .into_iter()
        .map(|(role, _)| role)
        .filter(|role| app_of(role).as_deref() != Some(role.as_str()))
        .map(|role| format!("{role}: app is {:?}", app_of(&role)))
        .collect();

    assert!(
        mismatched.is_empty(),
        "these roles compose `<role>{PARENT_DOMAIN_SUFFIX}`, but the CLI keys Computed \
         Vars by App: {mismatched:?}"
    );
}

/// Both halves of a `dns_record` call, from the same App's pair.
///
/// A Cloudflare token is zone-scoped (ADR-0068), so a domain from one Zone and
/// a token from another is a call that authenticates and then fails to find
/// the zone — or worse, finds it, when the fleet's token happens to be broad
/// enough. The pair is resolved together or it is not a pair.
#[test]
fn every_dns_record_call_passes_its_apps_pair() {
    let wrong: Vec<String> = dns_record_calls()
        .into_iter()
        .filter_map(|(role, name, vars)| {
            let expected = [
                (
                    RECORD_DOMAIN_PARAM,
                    format!("{{{{ {role}{PARENT_DOMAIN_SUFFIX} }}}}"),
                ),
                (
                    RECORD_TOKEN_PARAM,
                    format!("{{{{ {role}{DNS_TOKEN_SUFFIX} }}}}"),
                ),
            ];
            let bad: Vec<String> = expected
                .into_iter()
                .filter(|(param, want)| vars.get(*param) != Some(want))
                .map(|(param, want)| format!("{param}: {:?}, want {want:?}", vars.get(param)))
                .collect();
            (!bad.is_empty()).then(|| format!("{role} ({name}): {}", bad.join("; ")))
        })
        .collect();

    assert!(
        wrong.is_empty(),
        "a dns_record call writes into a zone its App did not resolve to: {wrong:?}"
    );
}

// ── The composition a scan cannot read ────────────────────────────────────
//
// Blocky's `customDNS` map is built by a `run_once` loop over every Playbook
// Meta in the tree, so its parent domain is named indirectly — `lookup('vars',
// app_name + '_parent_domain')` — and a text scan cannot tell reading that
// from reading it and throwing the answer away. That was the mutation which
// defeated #755's first fence, so the task's `vars:` chain is evaluated here
// against a written-out context and the FQDN it produces compared to a
// literal. `when:` is evaluated the same way, because "publishes nothing" is a
// claim about the guard and not about the expression.

/// The names this harness supplies rather than deriving: both are derivations
/// of the loop item that `blocky_tailnet_addresses.rs` answers for and this
/// file does not ask about.
const SEEDED: &[&str] = &["parsed", "app_name"];

/// The fleet's apex and the studio's, as the Computed Vars would carry them.
/// Written out, so an edit that moves which answer an entry composes against
/// has to move an assertion too (ADR-0046).
const FLEET_DOMAIN: &str = "example.com";
const STUDIO_DOMAIN: &str = "studio-example.com";

/// Render the way ansible's own jinja will: `trim_blocks` on, and undefined
/// strict — minijinja renders an unknown name as empty, which would turn every
/// FQDN below into a bare subdomain and leave the assertions passing over a
/// composition that reads nothing.
fn jinja() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    // `lookup('vars', name, default=…)`: ansible's own indirect read, and the
    // only way an expression can name a variable it computed. The default is
    // what makes an unresolved Zone answerable rather than fatal, so it is
    // honoured here exactly as ansible honours it.
    env.add_function(
        "lookup",
        |state: &State, plugin: String, name: String, kwargs: Kwargs| {
            assert_eq!(plugin, "vars", "the role looks up variables, not files");
            let fallback: Option<JValue> = kwargs.get("default").ok();
            kwargs.assert_all_used()?;
            Ok::<JValue, minijinja::Error>(
                state
                    .lookup(&name)
                    .filter(|value| !value.is_undefined())
                    .or(fallback)
                    .unwrap_or(JValue::UNDEFINED),
            )
        },
    );
    env
}

/// The task that accumulates the map, the one whose `vars:` chain composes the
/// FQDN. Absent is a hard stop: every assertion below is about this task.
fn accumulator() -> Task {
    let mut found: Vec<Task> = role_tasks("blocky")
        .into_iter()
        .filter(|task| {
            field(&task.body, "ansible.builtin.set_fact")
                .and_then(Value::as_mapping)
                .is_some_and(|facts| facts.contains_key(Value::from(ADDRESS_MAP)))
                && field(&task.body, "loop").is_some()
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one task must accumulate the tailnet-only map over the meta scan"
    );
    found.remove(0)
}

/// One task-local `vars:` entry, name to expression, in declaration order.
fn task_vars(task: &Task) -> Vec<(String, String)> {
    let vars = field(&task.body, "vars")
        .and_then(Value::as_mapping)
        .expect("the accumulator composes the FQDN in a `vars:` block");
    vars.iter()
        .filter_map(|(name, expr)| Some((name.as_str()?.to_string(), expr.as_str()?.to_string())))
        .collect()
}

/// The accumulator's `vars:` chain, evaluated against `context` in dependency
/// order — an entry that reads a name not yet bound is retried once the entry
/// binding it has run. A chain that never converges is a hard stop rather than
/// a partial context, since a missing binding reads as an empty FQDN.
fn evaluate(context: &BTreeMap<String, JValue>) -> BTreeMap<String, JValue> {
    let env = jinja();
    let task = accumulator();
    let mut bound = context.clone();
    let mut pending: Vec<(String, String)> = task_vars(&task)
        .into_iter()
        .filter(|(name, _)| !SEEDED.contains(&name.as_str()))
        .collect();

    while !pending.is_empty() {
        let before = pending.len();
        let mut blocked: Vec<String> = Vec::new();
        pending.retain(|(name, expression)| {
            match env.render_str(expression, JValue::from_serialize(&bound)) {
                Ok(rendered) => {
                    bound.insert(name.clone(), JValue::from(rendered));
                    false
                }
                Err(e) => {
                    blocked.push(format!("{name}: {e}"));
                    true
                }
            }
        });
        assert!(
            pending.len() < before,
            "the accumulator's `vars:` chain does not resolve; stuck on {blocked:?}"
        );
    }
    bound
}

/// Whether every `when:` clause standing over the accumulator holds — what
/// decides that an entry reaches the map at all.
fn guards_hold(bound: &BTreeMap<String, JValue>) -> bool {
    let env = jinja();
    let task = accumulator();
    assert!(
        !task.guards.is_empty(),
        "the accumulator must be guarded; an unguarded one maps every Meta in the tree"
    );
    task.guards.iter().all(|clause| {
        let rendered = env
            .render_str(
                &format!("{{{{ ({clause}) | bool }}}}"),
                JValue::from_serialize(bound),
            )
            .unwrap_or_else(|e| panic!("`when: {clause}` must evaluate: {e}"));
        rendered == "true"
    })
}

/// A Meta as the scan hands it over, plus the variables in scope on the Host
/// the infrastructure play targets.
fn context(meta: &str, app: &str, vars: &[(&str, &str)]) -> BTreeMap<String, JValue> {
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(meta).expect("the fixture Meta must parse as YAML");
    let mut context: BTreeMap<String, JValue> = BTreeMap::new();
    context.insert("parsed".to_string(), JValue::from_serialize(&parsed));
    context.insert("app_name".to_string(), JValue::from(app));
    for (name, value) in vars {
        context.insert((*name).to_string(), JValue::from(*value));
    }
    context
}

/// The FQDN and address one Meta lands in the map, or `None` where the guards
/// keep it out.
fn published(meta: &str, app: &str, vars: &[(&str, &str)]) -> Option<(String, String)> {
    let bound = evaluate(&context(meta, app, vars));
    if !guards_hold(&bound) {
        return None;
    }
    let read = |name: &str| {
        bound
            .get(name)
            .unwrap_or_else(|| panic!("the accumulator must bind `{name}`"))
            .to_string()
    };
    Some((read("app_fqdn"), read("app_address")))
}

const OFF_ZONE_META: &str = "subdomain: git\ntailnet_only: true\n";
const FLEET_META: &str = "subdomain: docs\ntailnet_only: true\n";

#[test]
fn an_app_in_a_second_zone_publishes_under_that_zones_apex() {
    assert_eq!(
        published(
            OFF_ZONE_META,
            "forgejo",
            &[
                ("domain", FLEET_DOMAIN),
                ("forgejo_parent_domain", STUDIO_DOMAIN),
                ("forgejo_tailscale_ip", "100.64.0.2"),
            ],
        ),
        Some((
            "git.studio-example.com".to_string(),
            "100.64.0.2".to_string()
        )),
        "the map composes off the App's resolved Zone, not the fleet's apex"
    );
}

#[test]
fn an_app_in_the_fleets_zone_publishes_under_the_fleets_apex() {
    assert_eq!(
        published(
            FLEET_META,
            "paperless",
            &[
                ("domain", FLEET_DOMAIN),
                ("paperless_parent_domain", FLEET_DOMAIN),
            ],
        ),
        Some(("docs.example.com".to_string(), String::new())),
        "every App in the fleet but one resolves to the fleet's Zone and must not move"
    );
}

/// The operator override still wins over the Meta's default, on both halves of
/// the composition: `<app>_subdomain` names the label, the Computed Var names
/// where the label sits.
#[test]
fn the_subdomain_override_composes_against_the_apps_zone() {
    assert_eq!(
        published(
            OFF_ZONE_META,
            "forgejo",
            &[
                ("domain", FLEET_DOMAIN),
                ("forgejo_parent_domain", STUDIO_DOMAIN),
                ("forgejo_subdomain", "forge"),
            ],
        )
        .map(|(fqdn, _)| fqdn),
        Some("forge.studio-example.com".to_string()),
    );
}

/// The property the others cannot state. Preflight refuses a run that deploys
/// an App whose Zone does not resolve, but this loop runs over *every* Meta in
/// the tree — including Apps this run does not touch — so an operator who
/// never onboarded a second Zone reaches here with the Computed Var absent.
/// The entry it would otherwise produce is `git.`, which blocky refuses to
/// load, taking every other Tailnet-only App's resolution with it.
#[test]
fn an_app_whose_zone_did_not_resolve_publishes_nothing() {
    assert_eq!(
        published(OFF_ZONE_META, "forgejo", &[("domain", FLEET_DOMAIN)]),
        None,
        "an App whose Zone has no answer on this Host must be left out of the map"
    );
}

/// The same guard, read from the other side: the fleet's own Apps are not
/// collateral of that exclusion.
#[test]
fn the_fleet_still_publishes_when_a_second_zone_is_unset() {
    assert!(
        published(
            FLEET_META,
            "paperless",
            &[
                ("domain", FLEET_DOMAIN),
                ("paperless_parent_domain", FLEET_DOMAIN)
            ]
        )
        .is_some(),
        "an operator with no second Zone must still publish the Apps that use the first"
    );
}

/// A Meta that is not Tailnet-only reaches no entry whatever Zone it is in.
#[test]
fn a_public_app_reaches_no_entry() {
    assert_eq!(
        published(
            "subdomain: git\n",
            "forgejo",
            &[
                ("domain", FLEET_DOMAIN),
                ("forgejo_parent_domain", STUDIO_DOMAIN)
            ],
        ),
        None,
        "Blocky's map is the Tailnet-only channel (ADR-0003)"
    );
}

/// The two ends of the Computed Var, held together across the language
/// boundary. The crate composes the name from [`PARENT_DOMAIN_SUFFIX`]; the
/// accumulator has to look up that same name, and must not have kept a
/// resolution of its own beside it — two resolvers agreeing is a thing a test
/// hopes for, which is what ADR-0081 deletes rather than fences.
#[test]
fn the_accumulator_looks_up_the_computed_var_the_crate_emits() {
    let expression = task_vars(&accumulator())
        .into_iter()
        .map(|(_, expression)| expression)
        .collect::<Vec<_>>()
        .join(" ");

    assert!(
        expression.contains(&format!("'{PARENT_DOMAIN_SUFFIX}'")),
        "the accumulator must compose the Computed Var's name off `{PARENT_DOMAIN_SUFFIX}`"
    );
    assert!(
        !expression.contains("domain_key"),
        "the accumulator must not resolve a Zone itself; the CLI resolved it already \
         and handed the answer down (ADR-0081). `domain_key:` is gone from the crate, but \
         `PlaybookMeta` takes unknown keys silently, so a Meta can still carry one — this \
         refuses the Ansible half of that resurrection (ADR-0071's founding failure)"
    );
    assert!(
        !expression.contains("zone"),
        "nor the live spelling. The assertion above forbids only the dead one, and a \
         resurrection would be written in this one. The needle is the bare word, so it also \
         refuses `host_zones` — the Host's token set is caddy's to read, and this task maps \
         one FQDN per App"
    );
}

// ── What still reads a Zone's key directly ────────────────────────────────

/// Every read of a Zone's Key Registry pair that survives under
/// `ansible/roles/`, as `(role, file, why it is not an App's own name)`.
///
/// Written out rather than counted, because the reason is the point: a count
/// says five and a reader still has to work out which five and why none of
/// them follows an App that changes Zone. A read that appears is refused
/// because nobody wrote down why; a row whose read is gone is refused because
/// the row outlived it, which is what keeps this a regime and not a pile of
/// excuses.
const DECLARED_ZONE_KEY_READS: &[(&str, &str, &str)] = &[
    (
        "caddy",
        "defaults/main.yml",
        "the fleet Zone's ACME token, the one line caddy's drop-in does not take from the \
         Host's Zone set: that set holds the fleet Zone on every Host, and the agent tier's \
         must not hold the parent domain's token (ADR-0068, ADR-0082). caddy_acme_token.rs \
         holds the per-Host choice",
    ),
    (
        "forgejo",
        "defaults/main.yml",
        "forgejo_admin_email is an address on the operator's mailbox, not a name this repo \
         publishes a record for; the forge changing Zone does not move where its admin reads mail",
    ),
    ("grimmory", "defaults/main.yml", "as forgejo"),
    (
        "headscale",
        "defaults/main.yml",
        "headscale_base_domain is the MagicDNS suffix — the tailnet's name, not headscale's own",
    ),
    (
        "headscale",
        "templates/headscale-config.yaml.j2",
        "the split-DNS entry maps the fleet apex, which no App changing Zone moves",
    ),
];

/// Both halves of every Zone the registry declares: the names a role must not
/// resolve for itself. Read off `keys.yml` rather than written out, so a Zone
/// added to the registry is one this walk looks for without anyone
/// remembering to add it.
fn zone_pair_keys() -> BTreeSet<String> {
    let token = &DNS_TOKEN_SUFFIX[1..];
    let keys: BTreeSet<String> = registry_keys()
        .into_iter()
        .filter(|key| {
            key == "domain"
                || key == token
                || key.ends_with("_domain")
                || key.ends_with(&format!("_{token}"))
        })
        .collect();
    assert!(
        keys.len() >= 2,
        "the registry must hold at least the fleet's pair: {keys:?}"
    );
    keys
}

/// Every identifier in a blob of text, so a name is matched whole. `domain`
/// must not hit `headscale_base_domain` or `dns_record_domain`, and a
/// substring search hits both.
fn identifiers(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            found.insert(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        found.insert(current);
    }
    found
}

/// The body of every `{{ … }}` and `{% … %}` in a file.
///
/// Reading only what is inside the delimiters keeps markup out structurally
/// rather than by exemption — the shape `variable_answerability` settled on. A
/// raw-text scan of this same tree reports blocky's task *name* ("tailnet-only
/// domain derivation") and paperless' ImageMagick `<policy domain="coder" …>`
/// as reads of the fleet's apex.
fn jinja_bodies(text: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    for (open, close) in [("{{", "}}"), ("{%", "%}")] {
        let mut rest = text;
        while let Some(start) = rest.find(open) {
            let after = &rest[start + open.len()..];
            let Some(end) = after.find(close) else { break };
            bodies.push(after[..end].to_string());
            rest = &after[end + close.len()..];
        }
    }
    bodies
}

/// Every `(role, file)` under `ansible/roles/` naming a Zone's registry pair
/// inside a Jinja expression. The playbooks are out of scope deliberately:
/// `infrastructure.yml` picks caddy's token per Host, and that choice is
/// `caddy_acme_token.rs`'s to hold.
fn zone_key_reads() -> BTreeSet<(String, String)> {
    let zone_keys = zone_pair_keys();
    let mut found = BTreeSet::new();
    for role in all_roles() {
        let dir = role_dir(&role);
        for path in role_yml_files(&role)
            .into_iter()
            .chain(role_template_files(&role))
        {
            let text = fs::read_to_string(&path).unwrap_or_default();
            let names: BTreeSet<String> = jinja_bodies(&text)
                .iter()
                .flat_map(|body| identifiers(body))
                .collect();
            if names.intersection(&zone_keys).next().is_some() {
                let relative = path
                    .strip_prefix(&dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                found.insert((role.clone(), relative));
            }
        }
    }
    found
}

/// The complement of the two walks above. A role naming a Zone's key directly
/// is a role resolving a Zone, and every one that survives had to be argued
/// for in `DECLARED_ZONE_KEY_READS`.
#[test]
fn every_surviving_read_of_a_zone_key_is_declared() {
    let declared: BTreeSet<(String, String)> = DECLARED_ZONE_KEY_READS
        .iter()
        .map(|(role, file, _)| ((*role).to_string(), (*file).to_string()))
        .collect();
    let found = zone_key_reads();

    let undeclared: Vec<&(String, String)> = found.difference(&declared).collect();
    assert!(
        undeclared.is_empty(),
        "these roles name a Zone's Key Registry entry directly, which is the CLI's job \
         (ADR-0081). Read the App's `{PARENT_DOMAIN_SUFFIX}` / `{DNS_TOKEN_SUFFIX}` instead, \
         or add a row to DECLARED_ZONE_KEY_READS saying why this one is not an App's own \
         name: {undeclared:?}"
    );

    let stale: Vec<&(String, String)> = declared.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "DECLARED_ZONE_KEY_READS names reads that are gone; drop the rows: {stale:?}"
    );
}
