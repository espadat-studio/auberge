---
title: "Forgejo"
---

Self-hosted git forge. Docs: [forgejo.org/docs](https://forgejo.org/docs/latest/), source: [codeberg.org/forgejo/forgejo](https://codeberg.org/forgejo/forgejo)

- **URL**: `https://{forgejo_subdomain}.{domain}` (default subdomain: `git`)
- **Data**: repositories, SQLite database and indexes under `/var/lib/forgejo/`; `app.ini` and the signing secrets under `/etc/forgejo/`
- **Pinned version**: 16.0.5

The reason to run one is identity. A Forgejo account is not a github.com account, so someone can hold write access to a repository without having one — which is what lets Decap CMS authenticate an editor against this forge.

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

## Notes

:::note
HTTPS only. Git over SSH is off — no `git` system user, no `authorized_keys`, and nothing listening beside the Host's hardened sshd. Clone and push over HTTPS. Forgejo Actions is off too.
:::

:::tip
The install wizard is locked and registration is disabled, so the administrator the first deploy creates is the only way in. Rotating `forgejo_admin_password` is not automatic — see the [role README](https://github.com/espadat-studio/auberge/blob/master/ansible/roles/forgejo/README.md#rotating-the-administrator-password).
:::

:::caution
Both paths are in the Backup Recipe and both are needed. `/etc/forgejo` holds three signing secrets that are generated once per Host and that no deploy can put back; restoring without them yields a forge that starts and serves every repository, with every session, 2FA enrolment and OAuth token void.
:::

:::caution
A push mirror **force-pushes** to its target on every sync. The mirrored remote is a replica, not a peer — anything pushed to it by hand is destroyed on the next sync. Push to the forge; let the mirror follow.
:::

For the OAuth application an editor logs in through, see the [role README](https://github.com/espadat-studio/auberge/blob/master/ansible/roles/forgejo/README.md#creating-the-oauth-application-for-an-editor). Decap reaches Forgejo through `decap-cms-backend-gitea`, not `decap-cms-backend-forgejo`.
