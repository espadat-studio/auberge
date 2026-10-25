# ADR-0087: hermes runs as its own unprivileged user

## Status

Accepted, 2026-10-25. Decided in security audit run 1, issue 21. Applies [ADR-0086](./0086-the-agent-tier-runs-as-its-own-unprivileged-user.md)'s reasoning to the hermes gateway.

## Decision

**The `hermes` role creates a `hermes` user and runs `hermes-gateway` as it. The user has no sudo, no supplementary groups and is not in `sshusers`.**

- `hermes_user` and `hermes_group` default to `hermes` in the role, not to `admin_user_name`. Only this role reads them, so they stay out of `group_vars/all`.
- The role creates the user itself with `groups: []` and `append: false`, so a standalone `hermes.yml` works and stray groups are removed.
- The first deploy after this change stops, disables and removes the admin user's old `hermes-gateway` unit and clears its failed state (ADR-0041). Two gateways on one bot token would fight over Telegram's long poll.
- The admin user's old `~/.hermes/` is left in place. Its venv has the admin home baked into its paths, so it is not moved. The owner copies sessions or memories across by hand.
- `auberge sync hermes` acts as `hermes` through the Host's `become_method`: rsync runs as `-u hermes`, and the restart goes through `systemctl --user --machine=hermes@`.
- The admin user's `NOPASSWD` sudo is unchanged. It is out of scope here and affects every Host workflow.

## Why

hermes takes instructions from Telegram and has a local terminal tool. Run as the admin user, it shared an identity with a sudo-capable user. The unit's `NoNewPrivileges=true` is a property of one process, not a boundary between identities. A separate user makes the boundary structural, as ADR-0086 did for the agent tier.

## Trade-off

- **Fresh state.** The gateway starts under `hermes` with no sessions or memories until the owner copies them.
- **The sync needs sudo.** `auberge sync hermes` now escalates on the Host, like every other CLI path that writes outside the ssh user's home.
- **The unit probe still looks in the ssh user's manager.** A failed deploy's unit report reads `scope: user` units as the ssh user, so it reports `hermes-gateway` as not found. `aoe` already has the same gap under `agent`.

## Consequences

`tests/hermes_user.rs` holds it: the role's runtime user is `hermes`, the role creates it, and no hermes task names a privileged group or touches sudoers. `tests/removed_unit_failed_state.rs` pins the retired unit's removal.
