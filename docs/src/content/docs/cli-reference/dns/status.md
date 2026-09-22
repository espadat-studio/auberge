---
title: "auberge dns status"
---

Show which configured subdomains have active Cloudflare A records and which are missing. Alias: `auberge d st`.

```bash
auberge dns status [OPTIONS]
```

Configured subdomains are discovered from `*_subdomain` keys in `config.toml` (e.g. `freshrss_subdomain`, `baikal_subdomain`).

## Options

| Option                | Description       | Default |
| --------------------- | ----------------- | ------- |
| `-o, --output FORMAT` | `human` or `json` | `human` |

## Examples

```bash
auberge dns status
auberge dns status
auberge dns status --output json
```

## Gotchas

- Missing subdomains are surfaced in both human and JSON output — the `missing_subdomains` field is the actionable signal.
- Fix missing records with `auberge dns set --subdomain <name> --ip <ip>` or bulk-set with `auberge dns set-all --host myserver`.
- `missing_subdomains` answers for one zone. An app in another Zone is in neither `configured_subdomains` nor `missing_subdomains`, so an empty `missing_subdomains` is an all-clear for this zone and not for the fleet. Those apps are named in `off_zone` (ADR-0081).

<details>
<summary>JSON output schema</summary>

```json
{
  "domain": "example.com",
  "configured_subdomains": ["blocky", "freshrss"],
  "active_a_records": [{ "name": "blocky", "ip": "192.168.1.10" }],
  "missing_subdomains": ["freshrss"],
  "off_zone": [{ "app": "forgejo", "subdomain": "git", "zone": "studio" }]
}
```

| Field                     | Type     | Description                            |
| ------------------------- | -------- | -------------------------------------- |
| `domain`                  | string   | Domain from config                     |
| `configured_subdomains`   | string[] | Subdomains from `config.toml`          |
| `active_a_records`        | object[] | A records present in Cloudflare        |
| `active_a_records[].name` | string   | Subdomain label                        |
| `active_a_records[].ip`   | string   | IP the record points to                |
| `missing_subdomains`      | string[] | Configured subdomains with no A record |
| `off_zone`                | object[] | Apps in a Zone this run does not hold  |
| `off_zone[].app`          | string   | App name                               |
| `off_zone[].subdomain`    | string   | Its effective subdomain                |
| `off_zone[].zone`         | string   | The Zone its record lives in           |

JSON goes to stdout; human-format chrome goes to stderr.

</details>
