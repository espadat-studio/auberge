# ADR-0085: CI falls back to a Standby Runner when hosted minutes run out

## Status

Accepted, 2026-10-22. Decided in #1111. Amended 2026-10-25: every job starts from a root-owned release and an empty HOME, and a root step does the self-update (Self-updating, below). Amended 2026-10-26: the runner is granted the whole install dir, with the release under `ReadOnlyPaths`, because a `ReadWritePaths` entry is its own mount even for the root step and `link()` fails across mounts; the reset empties the job dir in place instead of removing it (#1203). Extends [ADR-0054](./0054-agent-workloads-run-on-a-dedicated-disposable-host.md): `ruche` is still assumed compromisable, and the runner is shaped around that.

## Decision

**A self-hosted GitHub Actions runner on `ruche` takes over the secretless jobs of opted-in private repos when the org's hosted minutes run out, and hands them back when the month resets.** It is a standby, not the default path. A repo opts in by writing `runs-on: ${{ vars.CI_RUNNER || 'ubuntu-latest' }}` for each secretless job.

- **Secretless jobs only.** Lint, clippy, nextest, `check` and semantic-pr may move. Deploy and `content-sync` stay on hosted runners, so when minutes run out they stay blocked. A job whose output is deployed (a `build` producing the release artifact) counts as secret-bearing for this purpose: a compromised box could swap the artifact, so it stays hosted too. A runner on a compromisable box never holds `DEPLOY_SSH_KEY`, `CLOUDFLARE_API_TOKEN`, `GH_APP_PRIVATE_KEY` or `FORGEJO_TOKEN`.
- **One ephemeral runner, own user, no container socket.** It runs under an unprivileged user that is not `admin_user_name`, with CPU and memory caps. One instance, one job, re-registered per job. Nothing one job writes reaches the next: before every job a root step rebuilds the job dir from a root-owned release and empties HOME. All nine repos share one runner user, so per-job registration alone would let one repo's job read another's checkout or plant code a later job runs. Jobs do not use `container:`, so no Docker socket (root on the host) is exposed; the toolchain comes from mise.
- **Org-level, restricted.** One runner group, restricted to the nine private repos (a selected list, public repos disallowed). The group says which repos may use the runner, not which jobs: a job lands on it only when its workflow names the label, so "secretless only" is enforced by reviewing workflow changes, not by the group. Public repos never use it: a fork PR would run attacker code on the box ([#746](https://github.com/espadat-studio/auberge/issues/746)'s constraint).
- **A Runner Flip is a poller, not a human.** A systemd timer on `ruche` finds runs that failed with GitHub's billing error, sets `CI_RUNNER` to the runner's label, and re-runs each such run's failed jobs once. A real test failure is never re-run. On UTC day 1 it clears `CI_RUNNER`.
- **Its credential is a dedicated GitHub App, narrow by construction.** Org permission Self-hosted runners write; repository permissions Actions write, Variables write, Checks read (the billing annotation lives on the check run) and Metadata read; installed on the nine private repos (a selected list, so a new repo is not covered until added). No contents, pull-request, billing or secrets access. Its private key sits in a root-owned file the agent user cannot read, and installation tokens are minted per run. The existing deploy App is not reused: its key is a secret that must never reach `ruche`, and its permissions are far wider.
- **Self-updating, by root.** Before every job the root step moves the release to GitHub's latest, so the runner GitHub sees is never behind. GitHub stops queuing jobs to a runner more than 30 days behind any release, and a standby that is idle most months would be stale exactly when the flip needs it. This is a deliberate exception to the pinned-version rule. The step installs a download only if its sha256 matches the digest GitHub published for the asset. Job code cannot write the release: the job dir holds hard links to its root-owned, read-only files. No cache survives a job, so each job fetches its toolchain and crates again.
- **No offline alert.** Queued jobs and the runner list in org settings show an outage. Add a heartbeat if an outage goes unnoticed for more than a day.

## Why

The org runs out of the Free plan's 2,000 monthly private-repo minutes (1,988 used Oct 1–20, `recordreel` 1,232 of them). The shortfall is bursty, so a standby that absorbs the overshoot beats a runner that owns every run. Trimming triggers ([recordreel#159](https://github.com/espadat-studio/recordreel/issues/159), [#160](https://github.com/espadat-studio/recordreel/issues/160)) shrinks the bill without removing the next spike.

## Alternatives considered

- **Raise the spending limit.** About $6 for 1,000 extra minutes and no new component. Rejected by the maintainer in favor of owning the capacity. Still the cheapest fix if the standby proves a burden.
- **Poll the billing usage API and flip before minutes run out.** Proactive, with no failed first run. Rejected: the usage endpoints appear to need a classic personal access token, an org-admin-scale credential, on a box ADR-0054 assumes compromisable.
- **A runner for everything, always on.** Rejected: it would force secret-bearing jobs onto `ruche`, or a second trust tier.
- **The same runner as [#746](https://github.com/espadat-studio/auberge/issues/746)'s agentic lane.** Rejected: that lane needs a model key and a GitHub token. Opposite secret exposure; re-triage #746 against this record.
- **Rootless Podman or the Docker socket for `container:`.** Rejected: the socket is root; Podman adds a component. The jobs that move (`check`, semantic-pr) already use no `container:`, and the one job that does (`build`) stays hosted.
- **Let the runner update itself in place, as first deployed.** Rejected: the binaries had to be writable by the runner user, so job code could rewrite what later jobs from other repos run.
- **Keep the cargo registry and toolchain in HOME between jobs.** Rejected: a shared cache is a place to plant code for the next repo's job. The cost is a cold download per job.
- **Pin the runner with a checksum and Renovate (`--disableupdate`).** Rejected: the 30-day staleness rule turns a missed deploy into a silent queue at the worst moment.
- **Reuse the existing deploy GitHub App.** Rejected: it would put the key behind `GH_APP_PRIVATE_KEY` on `ruche`, with write access to contents and pull requests.
- **Per-repo runners.** Not needed: a Free org can restrict a runner group to selected repos, probed on 2026-10-22.

## Consequences

- The first billing-error failure per month is real. The poller re-runs it, so nobody re-runs it by hand. A re-run re-evaluates `runs-on` against the current variable, verified 2026-10-22. A billing failure is a job with zero steps, no runner name, and GitHub's billing annotation.
- If `ruche` is down the poller is down, and jobs fail with the billing error instead of queueing. That is fail fast, not a fallback.
- The poller's token can cancel or re-run workflows in all nine installed repos, not only the opted-in ones. That is the price of the re-run and of installing once.
- `landing-template` is not changed. Only `recordreel` opts in first.
- A green `check` from the Standby Runner can gate a deploy, since deploy `needs:` it. A compromised `ruche` could make `check` pass; the deploy's own hosted jobs still run the build.
