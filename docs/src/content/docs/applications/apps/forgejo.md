---
title: "Forgejo"
---

Self-hosted git forge. Docs: [forgejo.org/docs](https://forgejo.org/docs/latest/), source: [codeberg.org/forgejo/forgejo](https://codeberg.org/forgejo/forgejo)

- **URL**: `https://{forgejo_subdomain}.{domain}` (default subdomain: `git`), or `https://{forgejo_subdomain}.{<zone>_domain}` in [its own DNS zone](#its-own-dns-zone)
- **Data**: repositories, SQLite database and indexes under `/var/lib/forgejo/`; `app.ini` and the signing secrets under `/etc/forgejo/`
- **Pinned version**: 16.0.5

Run one for identity. A Forgejo account is not a github.com account, so someone can hold write access to a repository without one, which is how Decap CMS authenticates an editor against this forge.

## Deploy

```bash
auberge deploy forgejo
```

## Required config

| Key                      | Purpose                               |
| ------------------------ | ------------------------------------- |
| `forgejo_subdomain`      | Subdomain for HTTPS access            |
| `forgejo_admin_user`     | Administrator username                |
| `forgejo_admin_password` | Administrator password (first deploy) |

## Its own DNS zone

Forgejo can serve from a zone other than the fleet's. A zone is two keys sharing a prefix you choose: `<zone>_domain` and `<zone>_cloudflare_dns_api_token`. The fleet's zone is the unprefixed pair, `domain` and `cloudflare_dns_api_token`. `auberge config init` does not scaffold these keys, and the `auberge config set` picker does not list them, so set them by name:

```bash
auberge config set shop_domain shop-example.com
auberge config set shop_cloudflare_dns_api_token YOUR_TOKEN
```

The domain must be its own Cloudflare zone. Scope the token to that zone only, as for the [agent tier's token](/configuration/agent-tier-dns-zone/#provisioning).

Then point Forgejo at the zone with `auberge config edit`. Leave `forgejo_zone` unset to keep it in the fleet's zone.

```toml
[hosts.auberge]
forgejo_zone = "shop"
```

:::caution
`forgejo_zone` must sit under `[hosts.<name>]`. Preflight refuses it at the top level, because a Host's zones decide which ACME tokens land on its Caddy. The pair may sit at the top level or in the same host table. If either half is missing, preflight fails and names it.
:::

`auberge deploy forgejo` publishes the A record in `shop-example.com`. `auberge dns list`, `status`, `set-all` and `migrate` hold only the fleet's zone. They report Forgejo under `off_zone` and do not manage its record.

## Notes

:::note
HTTPS only. Git over SSH is off: no `git` system user, no `authorized_keys`, and nothing listening beside the Host's hardened sshd. Clone and push over HTTPS. Forgejo Actions is off too.
:::

:::tip
The install wizard is locked and registration is disabled, so the administrator the first deploy creates is the only way in. Rotating `forgejo_admin_password` is not automatic — see the [role README](https://github.com/espadat-studio/auberge/blob/master/ansible/roles/forgejo/README.md#rotating-the-administrator-password).
:::

:::caution
Both paths are in the Backup Recipe and both are needed. `/etc/forgejo` holds three signing secrets generated once per Host that no deploy can put back. Restore without them and the forge starts and serves every repository, with every session, 2FA enrolment and OAuth token void.
:::

:::caution
Onboarding a site for Decap creates a **separate content-only repository** on the forge. GitHub stays origin and nothing mirrors: a Forgejo push mirror runs `git push -f --mirror`, which deletes every branch that exists only on the target and closes its pull request. Renovate and template-sync branches are exactly that, and the per-branch filter does not prevent it — see [Why not migrate the site and mirror back](https://github.com/espadat-studio/auberge/blob/master/ansible/roles/forgejo/README.md#why-not-migrate-the-site-and-mirror-back).
:::

For the OAuth application an editor logs in through, see the [role README](https://github.com/espadat-studio/auberge/blob/master/ansible/roles/forgejo/README.md#creating-the-oauth-application-for-an-editor). Decap reaches Forgejo through `decap-cms-backend-gitea`, not `decap-cms-backend-forgejo`.
