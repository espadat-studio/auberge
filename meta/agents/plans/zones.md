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
> `effective_zone` read `zone:` **and** `domain_key:` at the time, because ADR-0081 kept both live; #953 retired `domain_key:` and left one branch. Phase 5 (#952, PR #958) shipped first and added a second resolver, `PlaybookMeta::zone()`, reading `domain_key:` only; its own tripwire fired the moment `<app>_zone` entered the registry, so phase 1 absorbed the convergence. `discover_all_subdomains_in` now calls `zone::publication_zone` and `PlaybookMeta::zone()` is gone. **`publication_zone` is the Host-free question**: `dns` holds one Zone per run and targets no Host, so an App placed under any `[hosts.<name>]` counts as off-Zone.

### Phase 2 — Injection (Rust)

| Commit                                                 | Changes                                 | Tests                 |
| ------------------------------------------------------ | --------------------------------------- | --------------------- |
| `feat(ansible): pass each app's zone as computed vars` | `computed_vars`; `ansible_runner.rs`    | `computed_vars.rs`    |
| `refactor(dns): app_parent_domain reads the zone`      | `services/dns.rs` delegates to `zone::` | existing dns.rs units |

Nothing in ansible reads them yet. Deployable, no-op.

> [!IMPORTANT]
> **Phase 2 landed as PR #959, with five deviations from the rows above.**
>
> - **The injection point is `Preflight`, not `ansible_runner.rs`.** `run_playbook`'s `extra_vars` is a per-call `-e` list; `Preflight::flat_vars` is the `@vars` file every run already writes. `preflight_for` overlays the Computed Vars there, _after_ the config flatten — `flatten_for_ansible` hands Ansible every top-level `config.toml` entry, Key Registry or not, so overlaying last is what makes a hand-written entry of one of these names reach no role rather than quietly win. `ansible_runner.rs` is untouched.
> - **`host_zone_set` is deleted.** Phase 1 added it; it never gained a production caller, and once `computed_vars` derived the same set it was a second derivation of one fact. Both questions now read one private walk, `placements()`, which returns each App with its Zone _and_ that Zone's pair. Its four unit cases moved onto `computed_vars`; one was fully covered by the new cases and dropped.
> - **The Host's Zone set is `host_zones`: JSON, `[{prefix, domain, token}]`, prefix `null` for the fleet's.** It carries the token beside the prefix so phase 4's template resolves nothing by name — a Zone is a pair, and an entry whose token the template looks up separately is the same pair split across two expressions again. **It commits to no `Environment=` spelling**, which is phase 4's to decide, as phase 1 left it.
> - **A contradicted pin fails _every_ run, not only the one deploying that App.** `computed_vars` walks all Metas, so a Meta `zone:` an operator's `<app>_zone` contradicts refuses a `deploy navidrome` too. Wider than plan C's per-run refusal, and deliberately: it is the same class as a fleet-wide `<app>_zone`, which `assert_no_fleet_wide_zone` already refuses fleet-wide. A config contradiction the operator has to resolve is not made smaller by deploying something else.
> - **An unanswered Zone yields _absence_, not an empty string.** Phase 3's roles will read an undefined var and fail the play, which is the intent: a role composing `{{ subdomain }}.{{ <app>_parent_domain }}` off `""` publishes `git.` silently. Preflight refuses the run first for any App the run deploys, so the undefined var is only reachable for an App this run does not touch.
> - **`app_verify_config` now skips an empty parent domain.** Pre-existing bug, made reachable by pair resolution: it composed `rss.` and failed the deploy with NXDOMAIN on a name no role ever tried to create. `services/dns.rs`'s old doc claimed the caller already handled this; it did not.
>
> `PlaybookMeta` derives `Default`, so `app_parent_domain` can resolve an App whose Meta will not load as an unpinned one — `<app>_zone` still places it. The Meta enumerator is the existing `playbook_meta::load_all_metas`, which is strict: a committed Meta that will not parse fails every run rather than silently dropping that App's Computed Vars and its `customDNS` entry.

### Phase 3 — Roles (Ansible)

| Commit                                                  | Changes                                    | Tests                                                                     |
| ------------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------- |
| `refactor(ansible): roles compose off their app's zone` | 17 defaults; 11 `dns_record` sites; blocky | `roles_compose_off_the_zone.rs`; replaces `tailnet_only_parent_domain.rs` |

> [!IMPORTANT]
> **Phase 3 landed as PR #961, with seven deviations from the row above.**
>
> - **18 defaults, not 17.** `aoe` composed off `{{ agents_domain }}` — the same defect spelled with the other Zone's key. Left alone it would have been the one role in the tree still resolving a Zone itself, which is the asymmetry this row's own rationale rejects. `aoe_parent_domain` resolved through the Meta's `domain_key:` to the same answer, so nothing moved; #953 re-spelled that pin `zone: agents`, again to the same answer.
> - **`infrastructure.meta.yml` keeps `domain`.** Phase 1 expected phase 3 to drop it. It cannot: headscale still reads `{{ domain }}` for `headscale_base_domain`, the MagicDNS suffix, and for the split-DNS entry in `headscale-config.yaml.j2`. Both are fleet-wide facts, not headscale's own name, and neither follows an App that changes Zone.
> - **Three more sites, because this change is what makes them wrong.** `yourls.caddyfile.j2` composed its site line off `{{ yourls_subdomain }}.{{ domain }}` where its six siblings read `{{ <app>_domain }}`; blocky's two Lego sites held the fleet's token for a certificate on `blocky_domain`; the 11 `dns_record` task `name:` strings composed the FQDN a third time. Each agreed with `<app>_domain` before this commit and could disagree after it, so they are consequences rather than adjacent cleanup.
> - **`variable_answerability.rs` learned what a Computed Var is.** It subtracts the Key Registry, `group_vars/` and the Meta-derived injections from what a run reads; a Computed Var is none of those, so all 29 new references read as names nothing can answer. The answer is gated on the App publishing a name — the Meta's `subdomain:`, or a `required_keys` entry demanding `<app>_subdomain`, which is the repo-side reading of `zone::publishes_a_name`. An App composing off a Zone it never publishes into still fails, verified by mutation.
> - **`headscale_derp_fallback.rs` seeds `headscale_parent_domain`.** It resolves the role's defaults to a fixpoint under a strict renderer, so the Computed Var has to sit beside its Key Registry answers rather than among them.
> - **The fence grew a third walk, because the two the issue specifies are shape-bound.** Review mutation-proved it: reverting `yourls.caddyfile.j2`'s site line _and_ blocky's Lego token left all 1539 tests passing, so the two sites the bullet above calls consequences had no fence at all. The third walk is the complement — every surviving read of a Zone's registry pair under `ansible/roles/`, written out with its reason, and an undeclared one refused. It is a declared regime rather than a drift check, so it also refuses a row whose read is gone, and an emptied walk fails by making all five rows stale. Reach is therefore three: 18 defaults, 11 `dns_record` sites, 5 declared reads.
> - **The deleted fence's surviving properties moved; none were dropped silently.** Its six Blocky evaluations are re-stated in `roles_compose_off_the_zone.rs` against the Computed Var. "Every declared `domain_key` is a registry key" was already held more strictly by `zone_declaration.rs`, which demands the pin name a Zone whose _pair_ the registry holds. "Only a Tailnet-only App declares a `domain_key`" is the one deliberate loss: its premise is what this work makes false. The cross-language assertion inverted — the accumulator must now name `PARENT_DOMAIN_SUFFIX` and must **not** mention `domain_key`.

### Phase 4 — Caddy (Ansible)

| Commit                                           | Changes                                                 | Tests                    |
| ------------------------------------------------ | ------------------------------------------------------- | ------------------------ |
| `feat(caddy): every vhost states its acme token` | 18 templates; `caddy-env.conf.j2`; `infrastructure.yml` | `vhost_acme_token.rs`    |
| `docs(adr): record per-site acme`                | ADR-0082; `adr.md`                                      | `check-adr-numbering.sh` |

> [!IMPORTANT]
> **Phase 4 landed as PR #964, with eleven deviations from the rows above.**
>
> - **The fleet's line is not taken from the Zone set, and ADR-0082 is amended for it.** A Zone is in a Host's set when an App that publishes a name resolves to it — and every App answers its `<app>_subdomain` fleet-wide, so the fleet Zone is in _every_ Host's set, the agent tier's included. Deriving that line writes the parent domain's token onto the one Host ADR-0068 exists to keep it off. `caddy_dns_api_token` stays the fleet Zone's line on every Host, chosen by `infrastructure.yml`; the set supplies the **named** Zones only.
> - **A named Zone also needs the Host's table to declare it serves the App**, so this phase changed `zone.rs` where the plan had it Ansible-only. A Meta's `zone:` holds on every Host, so the pin alone put the agent tier's token on all three boxes. `declared_on_host` reads `<app>_zone` or `<app>_subdomain` under `[hosts.<name>]` — the same declaration ADR-0072's gate already reads, and the only fact in the repo that says where an App runs.
> - **`Zone::env_var` is the Zone's token key uppercased**, not a spelling of its own. The fleet's comes out as `CLOUDFLARE_DNS_API_TOKEN` — what caddy already reads — rather than being asserted as a special case, which is where the drop-in and the vhost would drift apart.
> - **A third Computed Var, `<app>_dns_api_token_env`, carries the name to the vhost.** ADR-0082's `{env.<ZONE>_DNS_API_TOKEN}` can only be a literal, and a literal names the Zone the repo guessed; an operator moves an App from `config.toml`. Phase 1 cut `Zone::env_var` for having no caller — it has two now, the App's var and the Host's set, which is what keeps them one derivation.
> - **`infrastructure.yml` is untouched.** `host_zones` is in every run's `@vars` file, so the caddy role reads it like any other variable; there was nothing to feed.
> - **17 vhosts carry `tls`, not 18.** colporteur's second site is `http://localhost:<port>` over loopback — no name, no certificate, no challenge — and is a declared exemption the fence refuses once it grows one.
> - **`no_log: true` on the drop-in task.** `host_zones` carries a token per Zone under a name none of `config.rs`'s redaction suffixes match, which `services::zone` warned about when it was added; a `--diff` run would have printed all of them.
> - **Twelve vhosts change challenge type, and ADR-0082's "the behaviour change is nil" was wrong.** There is no global `acme_dns`: only the five tailnet-bound vhosts carried a `tls` block, and the other twelve were issuing over HTTP-01/TLS-ALPN, which naming a DNS provider disables. The token stops being irrelevant for them. The consequence is that the fleet's `Environment=` line is now **withheld** on a Host whose per-Host choice fell on a Zone it serves: the agent tier's Host holds the agents token, and a fleet vhost there would have answered DNS-01 with the wrong zone's credential, gone green, and failed at renewal. Nothing moves today — `[hosts.ruche]` withdraws both blocky and headscale, so ruche serves aoe alone. The ADR's amendment corrects the claim and the two counts with it.
> - **`vhost_acme_token.rs` is mutation-tested, 13 mutations, all caught**: a vhost reverting to the literal env var; a vhost losing its `tls` block; a vhost naming a sibling's variable; a vhost reading the token _value_; the drop-in taking the fleet's token from the set (twice, before and after the withholding guard); the walk narrowed to three roles; the exempt site gaining a challenge; an exemption outliving its file; an unquoted `Environment=`; the guard removed; the guard inverted; `selectattr('prefix')` dropped so every Zone reads as named.
> - **Two fences moved, neither narrowed.** `installed_units.rs` asserted the drop-in holds exactly one `Environment=`; it now names both lines, which also refuses an unrelated one. `aoe_dashboard_exposure.rs` renders the aoe vhost under a strict renderer and had to seed the new Computed Var beside the role's defaults — the shape phase 3 left in `headscale_derp_fallback.rs`.
> - **The ADR row is an amendment, not a record.** ADR-0082 landed with the design (f00b9dfc), so this phase amended it with what implementing it found.
>
> **Rollout order, before the cutover's step 3.** aoe's vhost stops reading `CLOUDFLARE_DNS_API_TOKEN` and starts reading `AGENTS_CLOUDFLARE_DNS_API_TOKEN`, which only the new drop-in writes. Deploy `infrastructure` on `ruche` before, or with, `aoe` — an App role deployed alone leaves its vhost naming a variable no drop-in defines, and caddy fails to start. The Ingress Gate catches it in the same run.

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

| Issue                                                    | Why deferred                                                                                                                                                                                                                                                                                |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Re-spell `domain_key:` as `zone:`, supersede ADR-0071    | **Done, #953.** ADR-0071 superseded by ADR-0081; `zone:` is the only spelling of the pin                                                                                                                                                                                                    |
| Name a vhost file after its App, not its FQDN (11 roles) | A rename mid-cutover means one reload where every site is new-and-old                                                                                                                                                                                                                       |
| `dns` subcommands go multi-zone                          | One off-Zone App does not pay for the surgery                                                                                                                                                                                                                                               |
| #960 — a guarded role's Zone is demanded nowhere         | **Decided, ADR-0083.** The guarded roster entry is flagged, not dropped; its Zone follows the config-answered `<app>_subdomain`, never the Meta's `subdomain:`. Widening to `publishes_a_name` was the trap — it refuses `deploy infrastructure` on every Host serving neither guarded role |

## F. Close-out

- PR review of the branch as another engineer would.
- Decide which review recommendations to take; apply them; iterate until tests pass.
- Remove any unnecessary comments introduced along the way.
