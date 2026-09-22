# ADR-0082: Caddy answers DNS-01 per site, not per process

## Status

Accepted, 2026-09-22. Amended 2026-09-22 — see the [Amendment](#amendment-2026-09-22-the-fleets-line-keeps-its-per-host-choice) below. Decided alongside [ADR-0081](./0081-an-apps-zone-is-a-named-pair-the-cli-resolves.md), which made an App's **Zone** a first-class thing. Revises [ADR-0072](./0072-the-agent-tiers-caddy-answers-for-its-own-zone.md), whose "one token per process, chosen per Host" holds only while a Host serves one Zone.

## Decision

**Every vhost states its own ACME token.** Each App's Caddyfile carries `tls { dns cloudflare {env.<ZONE>_DNS_API_TOKEN} }`, naming the token of the App's Zone. All twenty, not only the off-Zone one.

**The caddy role writes one `Environment=` line per Zone the Host serves.** The Host's Zone set is a **Computed Var**: the CLI already resolves each App's Zone, so it aggregates the ones answered for that Host and hands the set down. `caddy_dns_api_token` stays as the process-wide default for a Host with nothing to say.

**One writer for caddy's environment.** The drop-in is written by the caddy role in `infrastructure.yml`, as ADR-0072 established. No App role writes a fragment of its own.

## Why

ADR-0072 read the constraint as "caddy takes one token per process", and solved which token per Host. That was the whole problem while each Zone lived on its own Host — the agent tier on `ruche`, everything else on the parent domain. It stops being the whole problem the moment one Caddy terminates two Zones, which is what a studio forge beside the personal fleet is.

Always emitting the `tls` block is the part that looks like overreach and is not. "Absent means inherit the process default" is a hole a fence cannot see through: the assertion "every vhost's token comes from its App's Zone" could then only be checked against the vhosts that opted in, which is exactly the set that was already right. Emitting it on all twenty makes the assertion total. Nineteen of them name the value they already inherit, so the behaviour change is nil and the deploy is covered by the Ingress Gate either way.

A per-App systemd drop-in fragment was the tempting shape — no Host-to-Zone knowledge needed anywhere, since each role would write its own. It reproduces the stale-file bug this change already trips over: nothing removes a fragment when an App moves Zone or leaves, so a token outlives the App that justified it, on a box where the whole point was bounding which tokens live there.

## Amendment (2026-09-22): the fleet's line keeps its per-Host choice

The decision above has the caddy role write one `Environment=` line per Zone the Host serves, from the derived set. Implementing it showed the fleet's Zone cannot be one of those lines.

A Zone is in a Host's set when an App that publishes a name resolves to it and its pair answers there. Every App answers its own `<app>_subdomain` fleet-wide, so the fleet Zone is in **every** Host's set — the agent tier's included, and keeping the parent domain's token off that box is what ADR-0068 exists for. Deriving the fleet's line would have written it onto the one Host ADR-0054 assumes compromisable.

So `caddy_dns_api_token` is not the default for a Host with nothing to say. It is the fleet Zone's line, on every Host, chosen per Host by `infrastructure.yml` exactly as ADR-0072 has it. The Zone set supplies the **named** Zones only.

A named Zone needs one thing more than resolving: the Host's own table has to declare that Host serves the App — `<app>_zone` for one the operator placed, `<app>_subdomain` for one the repo pinned. A Meta's `zone:` holds on every Host, so the pin alone put the agent tier's token on all three boxes, and no declaration of which Host runs which App exists anywhere else in the repo. A Host serving an App its table never mentions writes no token for it and fails the Ingress Gate, which is the loud end of that trade.

Two spellings this decision left open, settled the same way — one rule, no literal:

- **The variable's name is the Zone's token key, uppercased.** `CLOUDFLARE_DNS_API_TOKEN` for the fleet, which is what caddy has read since ADR-0072 and is already on every Host; `STUDIO_CLOUDFLARE_DNS_API_TOKEN` for a named Zone. Writing `<ZONE>_DNS_API_TOKEN` as its own spelling would have needed the fleet's name as a special case, and a special case is where the two sides drift apart.
- **A vhost reads that name from its App's own Computed Var**, `<app>_dns_api_token_env`, not from a literal. An operator moves an App between Zones from `config.toml`, so a literal in a template could only ever name the Zone the repo guessed — the same argument ADR-0081 makes about the apex.

One vhost of the eighteen carries no `tls` block: colporteur's second site is `http://localhost:<port>` over loopback, with no name and no certificate. It is a declared exemption in `tests/vhost_acme_token.rs`, refused if it ever grows a challenge.

## Trade-off

- **A token's presence on a Host is now derived, not declared.** It follows from which Apps have a Zone answered for that Host, so an operator reading `config.toml` cannot see the drop-in's contents without resolving it. ADR-0081's refusal of a fleet-wide `<app>_zone` is what keeps that derivation from silently widening — without it, one un-scoped key puts a Zone's token on every Host.
- **Twenty Caddyfile templates change at once**, for a property nineteen of them already had implicitly.

## Alternatives considered

- **Emit `tls` only on off-Zone vhosts.** Rejected: the fence goes vacuous, as above. It also makes "which token does this site use" a question answered in two places depending on the site.
- **Write the token literally into the vhost.** Rejected: `/etc/caddy/sites/*.caddyfile` is mode `0644`.
- **A `caddy.service.d/<zone>.conf` fragment per App role.** Rejected: stale fragments, and a token outliving its App on precisely the Host whose token set is meant to be bounded.
- **Widen one Cloudflare token to cover both Zones.** Rejected: zero new code, and the wrong direction. The studio Zone holds business records — mail, the client-facing site — and the Host that would hold the widened token runs fifteen public Apps. ADR-0068's argument, pointed the other way.
- **CNAME-delegated ACME** (`_acme-challenge.git.<studio> CNAME _acme-challenge.git.<fleet>`, with `dns_challenge_override_domain`). Rejected: it removes the second token from the _certificate_ path only. The A record still needs a token for that Zone, so either the record is hand-made — outside DNS Publication, and drifting — or the token is on the box anyway and the delegation bought nothing.
- **Give the second Zone its own Host.** Rejected: a VPS, a second Caddy, a second backup driver and a second Ingress Gate, to avoid one `tls` block.
