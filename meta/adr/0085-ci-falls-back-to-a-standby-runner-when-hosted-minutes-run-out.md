# ADR-0085: CI falls back to a Standby Runner when hosted minutes run out

## Status

Accepted, 2026-10-22. Decided in #1111. Extends [ADR-0054](./0054-agent-workloads-run-on-a-dedicated-disposable-host.md): `ruche` is still assumed compromisable, and the runner is shaped around that.

## Decision

**A self-hosted GitHub Actions runner on `ruche` takes over the secretless jobs of opted-in private repos when the org's hosted minutes run out, and hands them back when the month resets.** It is a standby, not the default path. A repo opts in by writing `runs-on: ${{ vars.CI_RUNNER || 'ubuntu-latest' }}` for each secretless job.

- **Secretless jobs only.** Lint, clippy, nextest, `check` and semantic-pr may move. Deploy and `content-sync` stay on hosted runners, so when minutes run out they stay blocked. A runner on a compromisable box never holds `DEPLOY_SSH_KEY`, `CLOUDFLARE_API_TOKEN`, `GH_APP_PRIVATE_KEY` or `FORGEJO_TOKEN`.
- **One ephemeral runner, own user, no container socket.** It runs under an unprivileged user that is not `admin_user_name`, with CPU and memory caps. One instance, one job, re-registered per job. Jobs do not use `container:`, so no Docker socket (root on the host) is exposed; the toolchain comes from mise.
- **Org-level, restricted.** One runner group, restricted to the opted-in private repos. Public repos never use it: a fork PR would run attacker code on the box ([#746](https://github.com/espadat-studio/auberge/issues/746)'s constraint).
- **A Runner Flip is a poller, not a human.** A systemd timer on `ruche` finds runs that failed with GitHub's billing error, sets `CI_RUNNER` to the runner's label, and re-runs each such run's failed jobs once. A real test failure is never re-run. On UTC day 1 it clears `CI_RUNNER`.
- **Its credential is narrow by construction.** Actions read and write and Variables write on the opted-in repos. No billing, admin or secrets access. It sits in a root-owned file the agent user cannot read.
- **Pinned and self-contained.** Runner version pinned with a checksum and tracked by Renovate; `--disableupdate`. Ephemeral work dir, a capped cargo registry cache, a weekly prune.
- **No offline alert.** Queued jobs and the runner list in org settings show an outage. Add a heartbeat if an outage goes unnoticed for more than a day.

## Why

The org runs out of the Free plan's 2,000 monthly private-repo minutes (1,988 used Oct 1–20, `recordreel` 1,232 of them). The shortfall is bursty, so a standby that absorbs the overshoot beats a runner that owns every run. Trimming triggers ([recordreel#159](https://github.com/espadat-studio/recordreel/issues/159), [#160](https://github.com/espadat-studio/recordreel/issues/160)) shrinks the bill without removing the next spike.

## Alternatives considered

- **Raise the spending limit.** About $6 for 1,000 extra minutes and no new component. Rejected by the maintainer in favor of owning the capacity. Still the cheapest fix if the standby proves a burden.
- **Poll the billing usage API and flip before minutes run out.** Proactive, with no failed first run. Rejected: the usage endpoints appear to need a classic personal access token, an org-admin-scale credential, on a box ADR-0054 assumes compromisable.
- **A runner for everything, always on.** Rejected: it would force secret-bearing jobs onto `ruche`, or a second trust tier.
- **The same runner as [#746](https://github.com/espadat-studio/auberge/issues/746)'s agentic lane.** Rejected: that lane needs a model key and a GitHub token. Opposite secret exposure; re-triage #746 against this record.
- **Rootless Podman or the Docker socket for `container:`.** Rejected: the socket is root; Podman adds a component. Dropping `container:` for mise adds neither.
- **Per-repo runners.** Not needed: a Free org can restrict a runner group to selected repos, probed on 2026-10-22.

## Consequences

- The first billing-error failure per month is real. The poller re-runs it, so nobody re-runs it by hand; a re-run that does not pick up the new `runs-on` needs the poller to dispatch a fresh run instead, to be confirmed against a recorded sample.
- If `ruche` is down the poller is down, and jobs fail with the billing error instead of queueing. That is fail fast, not a fallback.
- The poller's token can cancel or re-run workflows in opted-in repos. That is the price of the re-run.
- `landing-template` is not changed. Only `recordreel` opts in first.
- A pin can go stale: GitHub refuses runners that fall too far behind the latest release. The exact window is unverified; Renovate has to keep pace.
