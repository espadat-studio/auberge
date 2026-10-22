# ADR-0086: The agent tier runs as its own unprivileged user

## Status

Accepted, 2026-10-22. Decided in #1142. Amends [ADR-0054](./0054-agent-workloads-run-on-a-dedicated-disposable-host.md)'s isolation claim and corrects [ADR-0065](./0065-the-agent-permission-baseline-is-a-guard-rail-not-a-boundary.md)'s "Root on the box" row.

## Decision

**A new `agent_users` role, first on `ruche.yml`'s roster, creates two users: `agent` and `syncthing`. Neither has sudo.**

- `agent` runs `aoe`, `opencode`, `github_identity` and `memsearch`. Its only supplementary groups are `systemd-journal` and `syncthing`. Not `sudo`, not `docker`, not `adm`.
- `syncthing` runs Syncthing on the `agent` group. It has no login shell.
- The admin user is unchanged and stays the Ansible and SSH identity.
- The names are `agent_user_name` and `agent_syncthing_user_name` in `group_vars/all`. Every role reads them from there, so a standalone `aoe.yml` or `memsearch.yml` resolves them without `agent_users` in the play.
- `group_vars/agent.yml` moves `syncthing_user` and `memsearch_user` for the `agent` group only. The roles keep their admin-user defaults, because `memsearch.yml` also runs on Hosts outside the group.
- The memory store is owned `agent:syncthing`, mode `2770`. The agent writes transcripts, Syncthing replicates them, and neither owns the other's files.
- `agent_users` installs a minimal apt baseline (`build-essential`, `pkg-config`, `libssl-dev`, `git`, `curl`, `jq`, `unzip`, `ripgrep`, `python3-venv`) and a pinned `mise`, so toolchains install per user.

## Why

ADR-0065 listed "the admin user's sudoers entry requires a password the agent does not hold" as the boundary against root. It is false: `roles/user` writes `NOPASSWD: ALL` for the admin user, and the agents ran as that user. The only thing between an injected agent and root was a deny rule ADR-0065 itself calls a guard rail. ADR-0054's "isolation must be structural, not behavioral" was therefore behavioral.

It also capped the box. A Standby Runner (ADR-0085) cannot hold a secret the agents must not read while the agents are root-equivalent.

Syncthing gets its own user because it replicates a directory the agent writes into. Run as the admin user, a compromised agent that could write a file Syncthing then served would hold the same identity as the process that can reach lechuck.

## Trade-off

- **No migration.** Users, homes and worktrees are new. The deploy rebuilds and re-clones, which ADR-0054 already priced in. Unpushed work in agent worktrees is lost.
- **`agent` cannot install system packages.** The apt baseline covers the common build inputs. Anything else is a deploy.
- **`adm` is not granted.** The agent reads the journal, not `/var/log` files. Add the group when a task needs one.
- **The names are not in the Key Registry.** The fence in `tests/required_keys_declarations.rs` treats a registered key a guarded role reads as one the run demands, which would make a defaulted name mandatory. Overriding a name is a `group_vars` edit until optional keys exist.
- **Standalone `aoe.yml` and `opencode.yml` assume the users exist.** They fail on a missing user rather than create one. `ruche.yml` is the way to build the Host.

## Consequences

`tests/agent_users.rs` holds it: no agent role defaults its user to `admin_user_name`, `agent_users` never names a privileged group or touches sudoers, `syncthing_user` moves for the `agent` group only, and the store is `agent:syncthing` `2770` on both ends.

The "Root on the box" boundary in ADR-0065 is now a user without sudo. A kernel or service exploit is still a way to root; the box stays assumed compromisable.
