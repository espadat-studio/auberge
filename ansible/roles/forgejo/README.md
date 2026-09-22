# Forgejo Ansible Role

Deploys [Forgejo](https://forgejo.org) — a Go single-binary, self-hosted git forge — as a systemd service behind Caddy, against SQLite.

The reason to run one is identity. A Forgejo account is not a github.com account, so someone can hold write access to a repository without one, and Decap CMS can authenticate an editor against this forge instead of GitHub.

## Shutdown

Forgejo catches `SIGTERM`, runs a graceful shutdown and exits **0**, verified against 16.0.5. So it needs no `SuccessExitStatus=` ([ADR-0038](https://github.com/espadat-studio/auberge/blob/master/meta/adr/0038-clean-shutdown-exit-status-is-declared-per-runtime.md)), and the nightly Backup Recipe stop leaves the unit `inactive` instead of latching it `failed`.

## Scope

Deliberately not deployed:

- **Git over SSH.** `DISABLE_SSH = true`, no `git` system user, no `authorized_keys`. Clone and push over HTTPS.
- **Forgejo Actions.** `ENABLED = false`; upstream turns the subsystem on by default.
- **Postgres, and any container.** One editor does not need either.

## Variables

| Variable                     | Default                                               | Description                                            |
| ---------------------------- | ----------------------------------------------------- | ------------------------------------------------------ |
| `forgejo_install_path`       | `/opt/forgejo`                                        | Binary install directory                               |
| `forgejo_data_dir`           | `/var/lib/forgejo`                                    | Repositories, SQLite database, indexes                 |
| `forgejo_config_dir`         | `/etc/forgejo`                                        | `app.ini` and the three signing secrets                |
| `forgejo_sys_user`           | `forgejo`                                             | System user                                            |
| `forgejo_port`               | `3043`                                                | Loopback port Caddy proxies to                         |
| `forgejo_domain`             | `{{ forgejo_subdomain }}.{{ forgejo_parent_domain }}` | Public hostname                                        |
| `forgejo_app_name`           | `Forgejo`                                             | Site name in the header and page titles                |
| `forgejo_admin_email`        | `{{ forgejo_admin_user }}@{{ domain }}`               | Email on the administrator account                     |
| `forgejo_version`            | from `forgejo.meta.yml`                               | Pinned upstream release, tracked by Renovate           |
| `forgejo_cors_allow_origins` | `""` (CORS off)                                       | Comma-separated origins granted CORS access, see below |

Required config keys: `forgejo_subdomain`, `forgejo_admin_user`, `forgejo_admin_password`.

## Why the config lives outside the data directory

`/etc/forgejo` is not in the unit's `ReadWritePaths`, so `ProtectSystem=strict` denies the service every path under it.

That matters because Forgejo rewrites `app.ini` itself in two places: `generateSaveInternalToken` when `INTERNAL_TOKEN` will not load, and `createSymmeticSigningKeyCfg` when the oauth2 JWT secret will not. Either one would leave the file on disk diverged from the template the deploy owns. Denied at the kernel, the service fails instead.

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
> Forgejo runs `git config --global` on startup and on several admin paths (`syncGitConfig`). With the wrong `HOME` it writes into the invoking user's global git config. That includes flipping `gpg.format`, which silently breaks commit signing. It has happened here. The unit sets `HOME`, and so does the role's own `admin user create`; a hand-run must too.

## Rotating the administrator password

`forgejo_admin_password` is applied once, on the deploy that creates the account. Changing it in `config.toml` and re-deploying does nothing: the create is gated on `/var/lib/forgejo/.admin_created`, because re-running it against an existing account fails the deploy.

Rotate deliberately instead:

```sh
sudo -u forgejo env HOME=/var/lib/forgejo /opt/forgejo/forgejo \
  --config /etc/forgejo/app.ini admin user change-password -u <user> -p '<new>'
```

## Onboarding a site for Decap

GitHub stays origin. Decap writes to a **separate, content-only repository on this forge**: it holds the client-editable copy and nothing else, shares no history with the site's source, and has no git relation to github.com. The site's own CI reads that repository, merges the editable keys into its source on GitHub, and deploys from there.

`examples/forgejo-onboard.sh` in this repository is the whole forge-side onboarding — it creates the content repository and registers the OAuth application described below. It migrates nothing and creates no mirror.

### Why not migrate the site and mirror back

Migrating the site's repository onto the forge and push-mirroring back to GitHub is the shape that suggests itself, and it destroys the repository it is pointed at.

> [!CAUTION]
> A push mirror runs `git push -f --mirror` unconditionally (`services/mirror/mirror_push.go`). `--mirror` marks for deletion **every advertised remote ref with no local counterpart**, and that code path never consults a refspec — so Forgejo's per-branch filter is not a mitigation. A branch that exists only on the target is deleted on the first sync.

Reproduced with two bare repositories and Forgejo's exact configuration, `remote add` plus a `master`-only push refspec:

```text
GitHub branches BEFORE:  master, renovate/all-minor-patch
git push -f --mirror gh   (branch filter = master only)
   - [deleted]         renovate/all-minor-patch
GitHub branches AFTER:   master
```

Every branch a bot owns lives only on GitHub, so every one of them is in that set. GitHub closes the pull request attached to a deleted branch, and what that costs depends on the title it closed:

- A grouped Renovate PR (`group:allNonMajor`) has no version in its title, so Renovate cannot use it as a cache key. It treats the PR as immortal and recreates it — an open/close loop for as long as the mirror runs.
- A single-dependency PR does carry a version, which Renovate caches. Closed once, it reads as declined and is **silently never offered again**.
- Template-sync PRs die the same way.

The replica discipline is the smaller cost and still real: from the first sync the GitHub repository is downstream, so anything pushed to it by hand is destroyed on the next one, with no merge and no warning.

Decided against for a live site, not in the abstract: it kept GitHub as origin for exactly this reason. That ADR lives in the site's own repository, which is private.

## Creating the OAuth application for an editor

Registration is disabled, so the administrator creates every account. For a Decap CMS editor:

1. **Settings → Applications → Manage OAuth2 Applications** on the forge, as the administrator.
2. Application Name: anything. Redirect URI: **the Decap admin page itself**, trailing slash included — `https://example.com/admin/`.
3. **Uncheck Confidential Client.**
4. Save. Copy the client ID. There is no secret to copy, and none is needed.

Then configure the site:

```yaml
backend:
  name: gitea
  base_url: https://git.example.com
  api_root: https://git.example.com/api/v1
  repo: owner/site-content
  branch: master
  app_id: <client ID from step 4>
```

### Why those fields

Decap reaches Forgejo through `decap-cms-backend-gitea`, not `decap-cms-backend-forgejo`. The gitea package has shipped since 2023-10; the forgejo one is newer, undocumented on decapcms.org, and not what Decap's own Forgejo instructions use. The minimum Forgejo for the path is 1.21.4.

Every default in that backend points at somebody else's server, and none of them is derived from another. Set all of them:

- **`base_url` is the forge root**, and only the login flow reads it. `PkceAuthenticator` appends `login/oauth/authorize` and `login/oauth/access_token` to it.
- **`api_root` is the forge root plus `/api/v1`**, and it is a separate setting that does _not_ fall back to `base_url`. Leave it out and the editor signs in to this forge, then reads and writes content on `https://try.gitea.io`.
- **`branch` defaults to `master`**, which is what the onboarding script initialises the content repository on. A repository on `main` fails to load until this is set.
- **The redirect URI is `origin + pathname` of the page Decap is served from**, not a broker callback — PKCE has no broker to call back to. `/admin/` and `/admin` are two different URIs; register the one the site serves.
- **Confidential Client must be off, because the token exchange sends no `client_secret`** — it carries `client_id`, `code`, `grant_type`, `redirect_uri` and `code_verifier`, and nothing else. Forgejo checks a secret only for a confidential application, and answers a missing one with `invalid_client` / `invalid empty client secret`. Unchecking it also puts the authorize endpoint in the mode Decap already speaks: PKCE is _required_ of a public client, and a public client is re-prompted for consent every time rather than silently re-granted.

Read off `decap-cms-backend-gitea` 3.5.2, `decap-cms-lib-auth` 3.3.2, and Forgejo 16.0.5 `routers/web/auth/oauth.go` (lines 503, 532, 773).

> [!NOTE]
> Decap [#7867](https://github.com/decaporg/decap-cms/issues/7867), "Impossible to login with forgejo — missing secret", is open with no comments since 2026-06-25. It is titled after that error: `invalid empty client secret` is what Forgejo returns to a **confidential** application whose token exchange carries no secret, and a secretless exchange is the only kind Decap's PKCE path can make. That diagnosis is read off both sources, not off a login — no editor has yet completed this flow against this forge ([#936](https://github.com/espadat-studio/auberge/issues/936)). Prove one before depending on the path.

## Cross-origin access for a browser-based client

Decap runs in the editor's browser on the _site's_ origin, not the forge's, so every call it makes — the OAuth token exchange and every later read/write through the API — is cross-origin from Forgejo's point of view. Forgejo serves no CORS headers by default, and the browser blocks the token exchange before Forgejo ever answers it: `TypeError: Failed to fetch` after the forge redirects back, not a rejection from the forge ([#945](https://github.com/espadat-studio/auberge/issues/945)).

Set `forgejo_cors_allow_origins` to every origin the admin page is served from, comma-separated, exactly as `redirects` is built for the OAuth application above — production, every preview deployment, and `localhost` if the login is exercised from a dev server. A single entry may carry one `*` wildcard (`https://*.example.pages.dev`) to cover a per-branch preview host. Leave it unset and CORS stays off, same as upstream's default.

> [!NOTE]
> Gitea's CORS middleware once covered `/api/v1` only. Verified against `routers/web/web.go` on current upstream: the `/login/oauth` group is wrapped in the same `optionsCorsHandler()` as the API, so enabling `[cors]` alone unblocks the token exchange — no separate Caddy-level header is needed.

## Memory

`forgejo.meta.yml` declares `MemoryHigh=400M` / `MemoryMax=600M`.

That comes from a measurement: **175 MiB idle RSS** for 16.0.5 on x86-64, against an empty SQLite instance that had served its web UI and its API. `VmHWM` tracked `VmRSS` throughout, so 175 MiB is the high-water mark as well.

Two caveats. It was taken on a development machine; the role had not been deployed when it was measured. And the instance held no repositories, so the number excludes whatever the repo tree and the bleve issue indexer add. Forgejo is a Go service, so RSS tracks the GC heap and grows with both. The budget is the measurement plus room for that: `MemoryHigh` is reclaim pressure, `MemoryMax` a ceiling, and neither is a target. Re-measure on the Host once it holds real repositories and move them if 400M turns out to be pressure rather than headroom.

## Upstream

Releases come from `code.forgejo.org`, Renovate's default registry for the `forgejo-releases` datasource. The pin in `forgejo.meta.yml` and `auberge versions --check-upstream` therefore resolve the same list, which is ADR-0080. Each asset ships a `.sha256` sidecar, and the role fetches it at deploy rather than carrying a literal: Renovate bumps the App Version and cannot recompute a digest beside it (ADR-0017).
