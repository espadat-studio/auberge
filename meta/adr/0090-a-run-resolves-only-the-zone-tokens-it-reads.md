# ADR-0090: A run resolves only the Zone tokens it reads

## Status

Accepted, 2026-11-14. **Amends [ADR-0089](./0089-a-run-resolves-only-the-bang-values-it-reads.md)**, whose "Zone tokens still reach every run" limit this removes. Amends [ADR-0081](./0081-an-apps-zone-is-a-named-pair-the-cli-resolves.md) and [ADR-0082](./0082-caddy-answers-dns-01-per-site.md), which computed every placed Zone's token on every run.

## Decision

`zone::computed_vars` takes the run's **Read Set**. A Zone's token resolves only when the run reads a Computed Var that carries it.

- **The trigger.** A token resolves when the Read Set is everything, contains `host_zones`, or contains `<app>_dns_api_token` for an App placed in that Zone. Otherwise its `!` command never runs.
- **Omit, never empty.** A run gets `<app>_dns_api_token` only if it reads that name, and `host_zones` only if it reads `host_zones`. If the walk ever misses a read, the role fails on an undefined var instead of sending Cloudflare an empty token.
- **Domains stay fleet-wide.** `<app>_parent_domain` and `<app>_dns_api_token_env` are emitted for every placed App on every run. Neither is a secret, and blocky's `customDNS` still covers every App by construction.
- **`host_zones` is all or nothing.** A run that reads it gets every Zone in the Host's set, each with its token. `caddy.env.j2` rewrites the whole drop-in from it, so a partial set would deploy green and break the other Zones' certificate renewals.
- `preflight_for` computes the Read Set once and passes the same value to `preflight_with_keys` and `computed_vars`.

## Why

After ADR-0089 a single-App deploy still ran every Zone's token command, because Computed Vars were overlaid after the Read Set filter. With a hardware-backed password manager, a `navidrome` deploy asked for the `agents` Zone's token too.

Only `infrastructure.yml` reads `host_zones`, and it already reaches blocky, so its Read Set is everything. The 15 App playbooks each read only their own App's token.

## Consequences

- A `navidrome` deploy resolves only the fleet Zone's token.
- A failing `!` on a token the run reads still fails Preflight, naming the key.
- A broken command on a Zone token no run reads goes unnoticed until a run reads it, as ADR-0089 already accepts for other keys.
