# Forgejo Ansible Role

Deploys [Forgejo](https://forgejo.org) — a Go single-binary, self-hosted git forge — as a systemd service behind Caddy, against SQLite.

The point of running one is identity: a Forgejo account is not a github.com account, so someone can hold write access to a repository without having one. That is what lets Decap CMS authenticate an editor against this forge instead of GitHub.

## Shutdown

Forgejo catches `SIGTERM`, runs a graceful shutdown and exits **0** — verified against 16.0.5, not assumed. It therefore needs no `SuccessExitStatus=` ([ADR-0038](https://github.com/espadat-studio/auberge/blob/master/meta/adr/0038-clean-shutdown-exit-status-is-declared-per-runtime.md)), and the nightly Backup Recipe stop leaves the unit `inactive` rather than latching it `failed`.

## Scope

Deliberately not deployed:

- **Git over SSH.** `DISABLE_SSH = true`, no `git` system user, no `authorized_keys`. Clone and push over HTTPS.
- **Forgejo Actions.** `ENABLED = false`; upstream turns the subsystem on by default.
- **Postgres, and any container.** One editor does not need either.

## Variables

| Variable               | Default                                 | Description                                  |
| ---------------------- | --------------------------------------- | -------------------------------------------- |
| `forgejo_install_path` | `/opt/forgejo`                          | Binary install directory                     |
| `forgejo_data_dir`     | `/var/lib/forgejo`                      | Repositories, SQLite database, indexes       |
| `forgejo_config_dir`   | `/etc/forgejo`                          | `app.ini` and the three signing secrets      |
| `forgejo_sys_user`     | `forgejo`                               | System user                                  |
| `forgejo_port`         | `3043`                                  | Loopback port Caddy proxies to               |
| `forgejo_domain`       | `{{ forgejo_subdomain }}.{{ domain }}`  | Public hostname                              |
| `forgejo_app_name`     | `Forgejo`                               | Site name in the header and page titles      |
| `forgejo_admin_email`  | `{{ forgejo_admin_user }}@{{ domain }}` | Email on the administrator account           |
| `forgejo_version`      | from `forgejo.meta.yml`                 | Pinned upstream release, tracked by Renovate |

Required config keys: `forgejo_subdomain`, `forgejo_admin_user`, `forgejo_admin_password`.

## Why the config lives outside the data directory

`/etc/forgejo` is not in the unit's `ReadWritePaths`, so `ProtectSystem=strict` denies the service every path under it.

That is not decoration. Forgejo rewrites `app.ini` itself in two places — `generateSaveInternalToken` when `INTERNAL_TOKEN` will not load, and `createSymmeticSigningKeyCfg` when the oauth2 JWT secret will not — and either one would leave the file on disk diverged from the template the deploy owns. Denied at the kernel, the service dies loudly instead.

The three secrets it would otherwise invent are generated once per Host by `forgejo generate secret`, each in the format Forgejo parses it back from, and read through `app.ini`'s `SECRET_KEY_URI` / `INTERNAL_TOKEN_URI` / `JWT_SECRET_URI` indirection. None of them is written into the template or asked of the operator. They are also why `/etc/forgejo` is in the Backup Recipe: nothing can put them back.

> [!IMPORTANT]
> A restore that drops `/etc/forgejo` yields a Forgejo that starts and serves every repository, and voids every session, 2FA enrolment and OAuth token on it. Restore both paths.

## Running the Forgejo CLI by hand

Always as the service user, with its `HOME` and its config:

```sh
sudo -u forgejo env HOME=/var/lib/forgejo GITEA_WORK_DIR=/var/lib/forgejo \
  /opt/forgejo/forgejo --config /etc/forgejo/app.ini admin user list
```

> [!WARNING]
> Forgejo runs `git config --global` on startup and on several admin paths (`syncGitConfig`). With the wrong `HOME` it writes into the invoking user's global git config — including flipping `gpg.format`, which silently breaks commit signing. Observed, not theoretical. The unit sets `HOME`, and so does the role's own `admin user create`; a hand-run must too.

## Rotating the administrator password

`forgejo_admin_password` is applied once, on the deploy that creates the account. Changing it in `config.toml` and re-deploying does nothing: the create is gated on `/var/lib/forgejo/.admin_created`, because re-running it against an existing account fails the deploy.

Rotate deliberately instead:

```sh
sudo -u forgejo env HOME=/var/lib/forgejo /opt/forgejo/forgejo \
  --config /etc/forgejo/app.ini admin user change-password -u <user> -p '<new>'
```

## Creating the OAuth application for an editor

Registration is disabled, so the administrator creates every account. For a Decap CMS editor:

1. **Settings → Applications → Manage OAuth2 Applications** on the forge, as the administrator.
2. Application Name: anything. Redirect URI: the callback of whatever OAuth broker the site uses.
3. **Uncheck Confidential Client.** Decap is a browser app and holds no client secret.
4. Save. The client ID is shown once; the secret is not needed.
5. On the site, use `backend: { name: gitea, ... }` — not `forgejo`. Decap reaches Forgejo through `decap-cms-backend-gitea`, which has shipped since 2023-10; `decap-cms-backend-forgejo` is newer, undocumented on decapcms.org, and not what Decap's own Forgejo instructions use. The minimum Forgejo for that path is 1.21.4.

> [!NOTE]
> Decap [#7867](https://github.com/decaporg/decap-cms/issues/7867) — "Impossible to login with forgejo — missing secret" — is open with no comments since June 2026. Unchecking Confidential Client is the documented step and the likely cause, but nobody has confirmed it. Prove a login before depending on this path.

## Push-mirroring

> [!CAUTION]
> A push mirror **force-pushes** to its target on every sync. The mirrored remote is a replica, not a peer: anything pushed to it by hand is destroyed on the next sync, with no merge and no warning. Push to the forge; let the mirror follow.

## Memory

`forgejo.meta.yml` declares `MemoryHigh=400M` / `MemoryMax=600M`.

Measured, not guessed: **175 MiB idle RSS** for 16.0.5 on x86-64, against an empty SQLite instance that had served its web UI and its API. `VmHWM` tracked `VmRSS` throughout, so that is the high-water mark too, not a trough.

Two caveats on that number. It was taken on a development machine, not on the deploy target — the role had not been deployed when it was measured — and the instance held no repositories, so it excludes whatever the repo tree and the bleve issue indexer add. Forgejo is a Go service, so RSS tracks the GC heap and grows with both. The budget is the measurement plus room for that: `MemoryHigh` is reclaim pressure, `MemoryMax` a ceiling, and neither is a target. Re-measure on the Host once it holds real repositories and move them if 400M turns out to be pressure rather than headroom.

## Upstream

Releases come from `code.forgejo.org`, which is Renovate's default registry for the `forgejo-releases` datasource, so the pin in `forgejo.meta.yml` and `auberge versions --check-upstream` resolve the same list. Each asset ships a `.sha256` sidecar; the role fetches it at deploy rather than carrying a literal, because Renovate bumps the App Version and cannot recompute a digest beside it (ADR-0017). Keeping the two registries the same is ADR-0080.
