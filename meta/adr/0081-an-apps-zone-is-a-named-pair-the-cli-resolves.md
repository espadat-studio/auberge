# ADR-0081: An App's Zone is a named pair, and only the CLI resolves it

## Status

Accepted, 2026-09-22. Amends [ADR-0071](./0071-a-tailnet-only-apps-parent-domain-is-per-app.md), which made a Tailnet-only App's parent domain per App and left the Public App half unplumbed. Superseded in part by the follow-up that re-spells `domain_key:` as `zone:`; until that lands, both spellings are live.

## Decision

**A Zone is a pair of Key Registry entries sharing a prefix — `<zone>_domain` and `<zone>_cloudflare_dns_api_token`.** The fleet's Zone is the unnamed one. `agents_domain` + `agents_cloudflare_dns_api_token` was already this shape; naming it makes a second one cost two registry entries instead of a mechanism.

**Which Zone an App is in has two declaration sites, because it is two different claims.** A Playbook Meta's `zone:` is the repo asserting an App must be isolated — true for every operator, and true of the agent tier alone (ADR-0068). `<app>_zone` in `config.toml` is the operator placing an App in a Zone of their own. A Meta pin **refuses** a Config override.

**`<app>_zone` must be host-scoped.** A Host's Zone set is what its Caddy holds ACME tokens for, so a fleet-wide answer writes a Zone's token onto every Host, `ruche` included. Preflight refuses the fleet-wide form.

**Preflight demands the effective Zone, not `domain`.** The twenty Metas that declared `domain` and `cloudflare_dns_api_token` drop both: no role reads either directly any more. `services::required_keys::preflight_for` instead demands that every App in the run has its effective Zone's pair resolving for the target Host — the fleet's when the App names none. `required_keys_for` stays a pure function of `(playbook, tags)`.

**No role resolves a Zone.** The CLI resolves it once and hands the answer down as **Computed Vars** — `<app>_parent_domain` and `<app>_dns_api_token`, for every App with a Meta, on every run. Roles compose against those; none composes against `domain`.

## Why

ADR-0071 named the failure precisely and then left half of it live: publish a name under one domain, verify it under another, report success for both. `src/commands/deploy.rs:382` already resolves the DNS Publication check through `services::dns::app_parent_domain` for **every** App, public and tailnet-only — so a role publishing `git.espadat.com` while the CLI verified `git.{domain}` would go green on a name nothing served. `git.{domain}` resolves to the Host today, so the check would not even have to be lucky.

A test asserting that two expressions agree is a weaker instrument than one value. Making the CLI the only resolver deletes the divergence class rather than fencing it: the role's vhost, the role's A record and the CLI's verification read the same string because it is the same string.

The two declaration sites are not redundancy. ADR-0068's isolation of the agent tier is a security invariant the repo states on every operator's behalf, and an operator who could unset it would re-create the leak. "The forge lives on the studio's domain" is the opposite: a fact about one deployment that the repo has no business asserting, and ADR-0071's "an unanswered key publishes nothing" rule would have left every other operator with no forge at all. One vocabulary, two authorities, and the repo's wins.

A Computed Var is not an Injected Key. An Injected Key is in the registry precisely so a stale `config.toml` value can be overridden (`tailscale_authkey`, ADR-0063). A resolved Zone has no such value to override — config naming one would be the third declaration site this ADR spent two paragraphs avoiding.

## What it costs

**Twenty roles change in lockstep.** Every `defaults/main.yml` composing `{{ <app>_subdomain }}.{{ domain }}` and every `dns_record` call site moves to the Computed Var. Nineteen of those diffs serve no need anyone has today; the alternative was one role that can move and nineteen that cannot, with nothing in the tree saying why.

**The first key that must be host-scoped.** Every other key is fleet-wide with `[hosts.<name>]` as an override. This one inverts that, and the reason is invisible at the point of writing it — hence the refusal rather than a doc line.

**A fence over a walk, which can pass vacuously.** "No role composes a public FQDN off `domain`" is only as good as the walk's reach, so the fence states its reach as two counts that drift when the tree changes, and is mutation-tested.

## Alternatives considered

- **Extend `domain_key:` to Public Apps.** Rejected: it borrows ADR-0071's wording and drops its precondition. The "unanswered publishes nothing" rule is right for an App that must be isolated and wrong for one that merely may be, and applying it to forgejo breaks the forge for every operator without a second zone.
- **A per-App `<app>_parent_domain` config key, with `<app>_cloudflare_dns_api_token` beside it.** Rejected: nothing ties the two, so a second App in the same Zone duplicates both values — ADR-0068's own rotation bug, where one of two names for a secret goes stale and a certificate quietly stops renewing.
- **A `[zones.<name>]` table in `config.toml`.** Rejected: the Key Registry knows no table but the reserved `[hosts.<name>]`, and `flatten_for_ansible` flattens tables to dotted scalars. A new config shape to express a pair the prefix convention already expresses twice.
- **Resolve the Zone in ansible**, through one shared expression in group vars keyed on `ansible_role_name`. Rejected: `include_role` rebinds that variable and `dns_record` is an included role, so the shared expression would read the wrong name at exactly the call site that publishes. It also leaves publish and verify as two expressions.
- **Resolve it per role**, with the `lookup('vars', …)` chain copied into twenty `defaults/main.yml`. Rejected: twenty copies of one expression is twenty places to drift, and it keeps the divergence class alive.
- **Make `required_keys_for` config-aware**, resolving `domain` to the Zone's pair per App. Rejected: it turns the required-key set from a pure function of `(playbook, tags)` into a function of config, which is a real loss in the one service whose purity the Preflight type depends on.
- **Leave `domain` and `cloudflare_dns_api_token` in the twenty Metas.** Rejected: after the Computed Vars land no role reads either, so every Meta would assert a dependency that stopped being true — the mirror of the bug `tests/injected_keys.rs` exists to prevent.
