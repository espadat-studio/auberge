---
title: "Stirling-PDF"
---

Self-hosted PDF editor. Docs: [docs.stirlingpdf.com](https://docs.stirlingpdf.com/), source: [github.com/Stirling-Tools/Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF)

- **URL**: `https://{stirling_subdomain}.{domain}` (default subdomain: `pdf`), reachable only over the tailnet
- **Data**: none. Working files live under `/run/stirling` (tmpfs) and vanish when the service stops
- **Pinned version**: 3.2.0

Draw a signature and add text boxes to a PDF from a phone browser. No login: the tailnet is the access control. Nothing a PDF touches outlives the request.

## Deploy

```bash
auberge deploy stirling
```

## Required config

| Key                  | Purpose                    |
| -------------------- | -------------------------- |
| `stirling_subdomain` | Subdomain for HTTPS access |

## Notes

:::note
The Meta has no `backup:` section on purpose. Server-side file storage, analytics, audit logging and the mobile-upload relay are switched off by environment variables in the unit, and saved signatures stay in the browser.
:::

:::caution
Anyone on the tailnet can use it, and the tailnet ACL is the only gate. Off-tailnet access needs a login, which this role does not configure.
:::
