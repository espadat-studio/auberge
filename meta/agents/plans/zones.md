# Plan: an App's Zone

Support a second DNS zone: most Apps on the fleet's domain, the forge on the studio's.

Decided in a `/mp:grill-with-docs` session, 2026-09-22. Records: [ADR-0081](../../adr/0081-an-apps-zone-is-a-named-pair-the-cli-resolves.md), [ADR-0082](../../adr/0082-caddy-answers-dns-01-per-site.md), `CONTEXT.md` (**Zone**, **Computed Var**). Delete this file once the work lands.

Layers: Rust CLI and Ansible only. No frontend, no middletier.

## Reach

| Thing                                            | Count |
| ------------------------------------------------ | ----- |
| Roles defining `<app>_domain` off `{{ domain }}` | 17    |
| `dns_record_domain: "{{ domain }}"` call sites   | 11    |
| Vhost templates (17 roles, colporteur has two)   | 18    |
| New Key Registry entries                         | 3     |
| New Computed Vars                                | 3     |

## A. File layout

### New

| Path                                  | Purpose                                                             |
| ------------------------------------- | ------------------------------------------------------------------- |
| `src/services/zone.rs`                | Zone resolution: effective Zone per App, pair lookup, Host Zone set |
| `tests/zone_declaration.rs`           | Meta `zone:` pin refuses a Config override; host-scope refusal      |
| `tests/computed_vars.rs`              | No Computed Var name collides with a Key Registry key               |
| `tests/roles_compose_off_the_zone.rs` | No role composes a public FQDN off `domain`; reach as two counts    |
| `tests/vhost_acme_token.rs`           | Every vhost's `tls` token is its App's Zone's                       |

### Changed

| Path                                              | Change                                                             |
| ------------------------------------------------- | ------------------------------------------------------------------ |
| `ansible/keys.yml`                                | `studio_domain`, `studio_cloudflare_dns_api_token`, `forgejo_zone` |
| `src/playbook_meta.rs`                            | `zone: Option<String>` field                                       |
| `src/services/dns.rs`                             | `app_parent_domain` delegates to `zone::` resolution               |
| `src/services/required_keys.rs`                   | `preflight_for` demands each App's effective Zone pair             |
| `src/config.rs`                                   | Refuse a fleet-wide `<app>_zone`                                   |
| `src/services/ansible_runner.rs`                  | Pass the Computed Vars as extra-vars                               |
| `src/commands/dns.rs`                             | Name off-Zone exclusions out loud                                  |
| 17 × `ansible/roles/*/defaults/main.yml`          | `{{ domain }}` → `{{ <app>_parent_domain }}`                       |
| 11 × `dns_record` call sites                      | Domain and token from the Computed Vars                            |
| 18 × vhost templates                              | `tls { dns cloudflare {env.<ZONE>_DNS_API_TOKEN} }`                |
| `ansible/roles/caddy/templates/caddy-env.conf.j2` | One `Environment=` line per Zone the Host serves                   |
| `ansible/roles/blocky/tasks/main.yml`             | Drop its own resolution; read the Computed Var                     |
| `ansible/playbooks/*.meta.yml` (20)               | Drop `domain` and `cloudflare_dns_api_token`                       |

### Deleted

| Path                                  | Why                                                          |
| ------------------------------------- | ------------------------------------------------------------ |
| `tests/tailnet_only_parent_domain.rs` | Its premise — public plus off-Zone is illegal — is now false |

## B. Structure

`src/services/zone.rs`

| Item                                             | Responsibility                                                     |
| ------------------------------------------------ | ------------------------------------------------------------------ |
| `struct Zone { prefix: Option<String> }`         | A Zone's identity. `None` is the fleet's.                          |
| `Zone::domain_key()` / `token_key()`             | The pair's two registry names                                      |
| `Zone::env_var()`                                | The `Environment=` name its token lands under                      |
| `effective_zone(meta, config, app, host)`        | Meta `zone:` pin, else `<app>_zone`, else the fleet's              |
| `resolve(zone, config, host) -> (domain, token)` | The pair's answers for one Host; `Err` when either is unanswered   |
| `host_zone_set(config, host, metas)`             | Every Zone answered for one Host — what its Caddy needs tokens for |
| `computed_vars(metas, config, host)`             | The `<app>_parent_domain` / `<app>_dns_api_token` / Host-set map   |

## C. Pseudocode

`effective_zone`

- Meta declares `zone:` → that Zone. If `<app>_zone` is also answered, **error**: the repo's pin is not overridable.
- Else `<app>_zone` answered for this Host → that Zone.
- Else the fleet's Zone.
- Failure: `<app>_zone` answered fleet-wide (not under `[hosts.…]`) → error naming the Host table it belongs in.

`resolve`

- Fleet Zone → `(domain, cloudflare_dns_api_token)`.
- Named `p` → `(p_domain, p_cloudflare_dns_api_token)`, both via `get_for_host`.
- Failure: either unanswered → error naming the Zone, the missing key and the Host.

`preflight_for` (extended)

- Resolve the run's Apps from the roster, as today.
- For each: `resolve(effective_zone(…))`. Any error fails the run before ansible.
- Then the existing `required_keys_for` path, unchanged.

`computed_vars`

- For every App with a Meta: `<app>_parent_domain`, `<app>_dns_api_token` from `resolve`.
- Plus the Host's Zone set, flattened to the `Environment=` names the caddy role writes.
- Every App, not just the run's: blocky builds its map `run_once` over all Metas.

## D. Tests

Runner: `mise r test` (cargo). Lint: `mise r lint` **and** `mise r lint-ansible` unscoped. `rm -rf ansible/.ansible` before trusting a local run.

### Unit — `src/services/zone.rs`

| Case                                                   | Expect                             |
| ------------------------------------------------------ | ---------------------------------- |
| No Meta pin, no `<app>_zone`                           | fleet Zone                         |
| `<app>_zone` under `[hosts.auberge]`, host = `auberge` | that Zone                          |
| same, host = `ruche`                                   | fleet Zone                         |
| Meta pin, no config                                    | pinned Zone                        |
| Meta pin **and** `<app>_zone`                          | error, names both sites            |
| `<app>_zone` at top level                              | error, names the `[hosts.…]` table |
| Named Zone, domain answered, token blank               | error, names the token key         |
| `host_zone_set` with one off-Zone App on one Host      | that Host: 2 Zones; others: 1      |

### Fences

| File                            | Asserts                                                                                                                |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `zone_declaration.rs`           | Meta pin refuses override; a fleet-wide `<app>_zone` is refused                                                        |
| `computed_vars.rs`              | No Computed Var name is a Key Registry key                                                                             |
| `roles_compose_off_the_zone.rs` | No role composes a public FQDN off `domain`. Reach: **17 defaults + 11 `dns_record` sites**, both asserted, both drift |
| `vhost_acme_token.rs`           | All 18 vhosts carry `tls`, each naming its App's Zone's env var                                                        |
| `injected_keys.rs` (existing)   | Still passes: `tailscale_authkey` is still the only Injected Key                                                       |

Mutation-test `roles_compose_off_the_zone.rs` and `vhost_acme_token.rs`: both walk the tree, and a narrowed walk passes for free.

### Before touching `tailnet_only_parent_domain.rs`

Dump its assertion domain, then dump the replacement's. A quietly smaller fence is the failure mode.

## E. Commits

### Phase 1 — Zone model (Rust + registry + docs)

| Commit                                                    | Changes                                                                                                 | Tests                    |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------ |
| `feat(config): a zone is a named pair of registry keys`   | `keys.yml` ×3; `zone.rs`; `playbook_meta.rs` `zone:`                                                    | the 8 unit cases         |
| `feat(config): preflight demands an app's effective zone` | `preflight_for`; `config.rs` fleet-wide refusal; drop `domain`/`cloudflare_dns_api_token` from 20 Metas | `zone_declaration.rs`    |
| `docs(adr): record the zone model`                        | ADR-0081; `CONTEXT.md` **Zone** + **Computed Var**; `adr.md`                                            | `check-adr-numbering.sh` |

> [!IMPORTANT]
> **Phase 1 landed as PR #957, with three deviations from the rows above.**
>
> - **Seven Metas declared `domain` or `cloudflare_dns_api_token`, not twenty.** Six dropped them. `infrastructure.meta.yml` keeps `domain` on purpose: blocky and headscale still compose against `{{ domain }}` behind `when:` guards, and an untagged run does not demand a guarded role's keys, so the Zone demand cannot reach them. **Phase 3 drops it**, once those two roles read the Computed Var.
> - **Only an App that publishes a name is asked for a Zone** (`zone::publishes_a_name`). Demanding the fleet's pair of every Meta breaks `deploy ruche` and `bootstrap`. Recorded in `CONTEXT.md` under **Zone**.
> - **`Zone::env_var` was cut.** It had no caller and its spelling contradicted ADR-0082's `<ZONE>_DNS_API_TOKEN`. **Phase 4 decides the name** with the drop-in in front of it; the fleet's must stay `CLOUDFLARE_DNS_API_TOKEN`.
>
> `effective_zone` reads `zone:` **and** `domain_key:`, because ADR-0081 keeps both live. Phase 5 (#952, PR #958) shipped first and added a second resolver, `PlaybookMeta::zone()`, which reads `domain_key:` only. **Phase 2 must delete it** rather than leave `dns` and Preflight disagreeing about which Zone an App is in.

### Phase 2 — Injection (Rust)

| Commit                                                 | Changes                                 | Tests                 |
| ------------------------------------------------------ | --------------------------------------- | --------------------- |
| `feat(ansible): pass each app's zone as computed vars` | `computed_vars`; `ansible_runner.rs`    | `computed_vars.rs`    |
| `refactor(dns): app_parent_domain reads the zone`      | `services/dns.rs` delegates to `zone::` | existing dns.rs units |

Nothing in ansible reads them yet. Deployable, no-op.

### Phase 3 — Roles (Ansible)

| Commit                                                  | Changes                                    | Tests                                                                     |
| ------------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------- |
| `refactor(ansible): roles compose off their app's zone` | 17 defaults; 11 `dns_record` sites; blocky | `roles_compose_off_the_zone.rs`; replaces `tailnet_only_parent_domain.rs` |

### Phase 4 — Caddy (Ansible)

| Commit                                           | Changes                                                 | Tests                    |
| ------------------------------------------------ | ------------------------------------------------------- | ------------------------ |
| `feat(caddy): every vhost states its acme token` | 18 templates; `caddy-env.conf.j2`; `infrastructure.yml` | `vhost_acme_token.rs`    |
| `docs(adr): record per-site acme`                | ADR-0082; `adr.md`                                      | `check-adr-numbering.sh` |

### Phase 5 — CLI surface

| Commit                                           | Changes           | Tests                            |
| ------------------------------------------------ | ----------------- | -------------------------------- |
| `fix(dns): name the off-zone apps set-all skips` | `commands/dns.rs` | exclusion is printed, not silent |

### Cutover (an operation, not a PR)

1. Mint an `espadat.com`-scoped Cloudflare token (`Zone:DNS:Edit` + `Zone:Zone:Read`).
2. Answer `studio_domain`, `studio_cloudflare_dns_api_token`; set `forgejo_zone = "studio"` **under `[hosts.auberge]`**.
3. `auberge deploy forgejo`. New vhost, new cert, new A record. `git.{fleet}` still served — the stale vhost file is the overlap window.
4. Re-run `examples/forgejo-onboard.sh` per client with `FORGEJO_URL=https://git.espadat.com`.
5. Re-point local git remotes.
6. Check whether any Forgejo OAuth application's redirect URI names the forge itself rather than a client site; re-issue if so.
7. Close the window: `rm /etc/caddy/sites/git.{fleet}.caddyfile`, reload caddy, `auberge dns delete --subdomain git`.

### Follow-up issues

| Issue                                                    | Why deferred                                                          |
| -------------------------------------------------------- | --------------------------------------------------------------------- |
| Re-spell `domain_key:` as `zone:`, supersede ADR-0071    | Touches live agent-tier behaviour; not driven by this change          |
| Name a vhost file after its App, not its FQDN (11 roles) | A rename mid-cutover means one reload where every site is new-and-old |
| `dns` subcommands go multi-zone                          | One off-Zone App does not pay for the surgery                         |

## F. Close-out

- PR review of the branch as another engineer would.
- Decide which review recommendations to take; apply them; iterate until tests pass.
- Remove any unnecessary comments introduced along the way.
