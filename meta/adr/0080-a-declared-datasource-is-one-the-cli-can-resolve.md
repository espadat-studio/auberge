# ADR-0080: A declared datasource is one the CLI can resolve

## Status

Accepted, 2026-09-21. Arose from #933 (Forgejo), whose upstream is not GitHub. Extends [ADR-0017](./0017-app-versions-declared-in-playbook-meta.md), which decided that an App Version is declared once in the Playbook Meta and read by two consumers; this records what that shared declaration obliges each consumer to support.

## Decision

**A `version:` block's `datasource:` must be one `UpstreamClient::latest` has an arm for, and a test enforces it.** `every_declared_datasource_has_an_arm_in_this_client` puts every declared pin — App Versions from the Metas and Tool Versions from the role annotations — through the real `latest` against a base nothing listens on. A supported arm fails with a transport error; an unsupported one fails before the request, and the fence names the App and the datasource.

**A new datasource arm lands in the same change as the first Meta that declares it.** Alone it is a code path with no consumer, which nothing can hold to the vocabulary it was written for.

**An arm resolves against the same registry Renovate defaults to.** `forgejo-releases` therefore queries `code.forgejo.org`, not `codeberg.org`, although Forgejo develops on the latter and both serve every release. The two readers of a pin then resolve the same list without a `registryUrls` override to keep in step.

## Why

ADR-0017 put the pin in one place and pointed two readers at it: Renovate, which raises the bump, and `auberge versions --check-upstream`, which reports drift. The two have never had the same vocabulary. Renovate knows dozens of datasources; the CLI knew three — `npm`, `github-releases`, `go` — and `eyre::bail!`ed on the rest.

That bail is not scoped to the App that caused it. `app_drift_reports` propagates with `?` inside its loop, and `versions_and_report` does the same on the whole vector, so **one** Meta naming an unknown datasource makes the command report nothing for **any** App. The drift report is the fleet's only standing signal that a pinned App has fallen behind, and it would have gone dark on a change that was locally correct: `datasource: forgejo-releases` is exactly what Renovate needs, and it is what would have silenced the other eighteen Apps.

Nothing would have caught it. The declaration is valid YAML, the Meta parses, every other test passes, and `--check-upstream` is not run in CI — it queries the network. The failure surfaces the next time a human runs the command, with no indication that one App's declaration is the cause.

The fence is behavioural rather than a list of supported names. A `SUPPORTED_DATASOURCES` constant matched against the Metas would pass whenever the constant and the `match` agreed with each other, which is the fence reading its own expectation. Driving the real `latest` means the arm has to exist for the call to reach the network at all, and the two errors are distinguishable because one is raised before any request is made.

## Consequences

- A Meta cannot declare an upstream the CLI cannot follow. That is a real constraint: an App published somewhere exotic now costs an arm as well as a role, and the arm is not optional work that can be deferred to a follow-up.
- `UpstreamClient::with_bases` grows a parameter per datasource that has a registry. Four is still legible; a fifth or sixth should become a struct rather than a longer tuple of `String`s.
- The fence runs offline and in about 30ms, so it costs nothing to keep. It does not check that an arm is _correct_ — only that it exists. Each arm still carries its own wiremock tests for extraction, ordering and stability.
- Renovate remains free to use a datasource the CLI lacks for something that is not an App or Tool Version — the GitHub Actions and npm managers are untouched by this. The contract binds the `version:` blocks and the `# renovate:` annotations, which are the two the CLI reads.

## References

- `src/commands/versions.rs` — the arms and the fence.
- [ADR-0017](./0017-app-versions-declared-in-playbook-meta.md) — one declaration, two consumers.
- Renovate's [`forgejo-releases` datasource](https://docs.renovatebot.com/modules/datasource/forgejo-releases/), whose default `registryUrl` is `https://code.forgejo.org`.
