---
title: "auberge dns set-all"
---

Batch-create Cloudflare A records for all configured app subdomains. Alias: `auberge d sa`.

```bash
auberge dns set-all [OPTIONS]
```

## Options

| Option                   | Description                           | Default     |
| ------------------------ | ------------------------------------- | ----------- |
| `-H, --host HOST`        | Target host                           | Interactive |
| `-i, --ip IP`            | Override IP (conflicts with `--host`) | From host   |
| `-n, --dry-run`          | Preview without creating              | `false`     |
| `-y, --yes`              | Skip confirmation                     | `false`     |
| `-s, --strict`           | Fail if any subdomain env var missing | `false`     |
| `-S, --subdomains NAMES` | Process only these subdomains         | All         |
| `--skip NAMES`           | Exclude these subdomains              | None        |
| `--continue-on-error`    | Continue past errors                  | `false`     |
| `-o, --output FORMAT`    | `human` or `json`                     | `human`     |

## Examples

```bash
auberge dns set-all                                              # interactive
auberge dns set-all --host my-vps --dry-run
auberge dns set-all --host my-vps --subdomains freshrss,baikal
auberge dns set-all --host my-vps --skip calibre,yourls --yes
auberge dns set-all --host my-vps --strict                       # CI: fail on missing config
```

## Subdomain discovery

Reads `*_subdomain` keys from `config.toml` (e.g. `freshrss_subdomain`, `baikal_subdomain`). Set them with `auberge config set <app>_subdomain <name>`.

## Apps this run does not write

Two kinds, and neither is left out of the count — both are named in the report, with the reason that applies.

**Tailnet-only.** Playbook meta declares `tailnet_only: true` (currently `bichon`, `cockpit`, `paperless`). DNS is published via Blocky's `customDNS` map (ADR-0003), never via Cloudflare. Correct it with `auberge deploy <app>`.

**Off-zone.** The app's Zone is not the one this run holds. `dns` resolves one zone per run, so the record is out of reach rather than absent (ADR-0081). Correct it with a run scoped to that zone.

| Source                              | Behavior                                                       |
| ----------------------------------- | -------------------------------------------------------------- |
| Implicit (no `--subdomains`)        | Skipped; each app listed under `Skipping:` with its own reason |
| Explicit (`--subdomains` names one) | Hard-error before any record is written, naming the offender   |

The explicit branch errors rather than skipping because a name the operator typed is a choice they own. Writing the rest of the list and reporting success would answer a request that was never honoured.

:::caution
A 500 ms delay is inserted between API calls to respect Cloudflare rate limits.
:::

## Exit codes

Follow the Backup Verdict convention, so a script can branch on which of the three happened:

| Code | Meaning                                                                                                                                                                                                 |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `0`  | Every planned record written — including a run with nothing to do, a `--dry-run`, and a cancelled confirmation                                                                                          |
| `1`  | At least one write failed; the failures are in the `failed` array                                                                                                                                       |
| `2`  | Operational error — `--strict` without `--host`/`--ip`, no Host pickable (several off-terminal, or none configured), host absent from inventory, a tailnet-only or off-zone app named in `--subdomains` |

Under `--output json`, every path the run can end on emits a body (ADR-0044) — a failed write before exiting `1`, a dry run, and a declined confirmation alike:

```bash
auberge dns set-all --host my-vps --yes --output json --continue-on-error > records.json || echo "some records failed"
auberge dns set-all --host my-vps --dry-run --output json | jq '.planned'
```

<details>
<summary>JSON output schema</summary>

One shape on every path. `outcome` says what the run did with its plan; `planned` always holds the full plan.

```json
{
  "outcome": "applied",
  "planned": [
    {
      "app": "freshrss",
      "subdomain": "rss",
      "fqdn": "rss.example.com",
      "ip": "203.0.113.10"
    }
  ],
  "created": [
    {
      "subdomain": "rss",
      "fqdn": "rss.example.com",
      "ip": "203.0.113.10",
      "success": true
    }
  ],
  "skipped": [
    { "app": "bichon", "subdomain": "bichon", "reason": "tailnet_only" },
    {
      "app": "forgejo",
      "subdomain": "git",
      "reason": "off_zone",
      "zone": "studio",
      "run_domain": "example.com"
    }
  ],
  "failed": []
}
```

| Field              | Contents                                             | Description                                                                               |
| ------------------ | ---------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `outcome`          | `"applied"` \| `"dry_run"` \| `"cancelled"`          | What the run did with its plan — branch here, not on array emptiness                      |
| `planned`          | `app`, `subdomain`, `fqdn`, `ip`                     | The full plan with effective IPs, on every outcome — the denominator for the arrays below |
| `created`/`failed` | `subdomain`, `fqdn`, `ip`, `success`, `error?`       | Operation result per app; both empty unless `outcome` is `applied`                        |
| `skipped`          | `app`, `subdomain`, `reason`, `zone?`, `run_domain?` | Every app the plan left alone. `reason` is `"tailnet_only"` or `"off_zone"`               |

- `outcome: "applied"` — the plan ran; `created` and `failed` partition `planned`.
- `outcome: "dry_run"` — nothing was written; the plan is under `planned`, and `created`/`failed` are empty.
- `outcome: "cancelled"` — the `Proceed?` confirmation was declined; same body as a dry run. A non-interactive caller that forgets `--yes` lands here (the prompt refuses off-terminal), so a non-empty `planned` with `outcome: "cancelled"` is the "you forgot `--yes`" signal.

`zone` and `run_domain` appear only on an `off_zone` row, and name the app's Zone and the one the run holds. A tailnet-only row carries neither: a record kept off Cloudflare on principle has no zone to name. Branch on `reason` — only `off_zone` is fixable by running somewhere else.

All arrays are sorted alphabetically by app name. JSON to stdout; chrome to stderr.

</details>
