# ADR-0083: A guarded role's Zone follows its serving gate

## Status

Accepted, 2026-09-22. Completes [ADR-0081](./0081-an-apps-zone-is-a-named-pair-the-cli-resolves.md), whose "Preflight demands the effective Zone of every App in the run" was true of every App the run's source list reached — and that list reached no `when:`-guarded role.

## Decision

**A role the run reaches only from behind a `when:` guard is asked whether its serving gate is answered, not whether it publishes a name.** The gate is `<app>_subdomain` in `config.toml`, scoped to the Host. When it is answered, the App's effective Zone must resolve for that Host or Preflight refuses the run by name.

**A guarded roster entry is flagged, not dropped.** `run_sources` returns every entry an untagged run sweeps, each carrying whether a guard stands between the run and it. Key demands still refuse to look behind a guard, as [ADR-0045](./0045-required-keys-are-declared-in-playbook-meta.md) established. The Zone demand reads the same flag and asks the narrower question. One walk, two readings.

**A guarded role that publishes a name carries no `<app>_subdomain` role default.** Ansible's own gate reads the merged variables, defaults included; the Zone demand reads config alone. A default satisfies the first and not the second, so the role would run on a Host Preflight asked for no Zone. `aoe` carried one; it is dropped here, and `tests/guarded_role_serving_gate.rs` refuses the next.

**Naming a role by tag removes the guard from the question.** The operator asserting a role runs is a stronger claim than any gate, so a tagged run demands the Zone of whatever the Meta says publishes a name.

## Why

"Publishes a name" ORs two claims that answer different questions. A Meta's `subdomain:` is a repo fact: it is true on every Host, because it states the name the App _would_ serve once it runs. The `<app>_subdomain` answer is the operator turning the App on for one Host (ADR-0051, ADR-0058). Only the second can decide whether a guarded role runs — and it is the one the guard itself reads.

The gap was invisible because it was covered from the other side until it wasn't. Before ADR-0081, `blocky_domain` composed off `domain`, which `infrastructure.meta.yml` demands outright, so no guarded role needed a Zone of its own. Once the roles compose off `<app>_parent_domain` — a Computed Var emitted only when the Zone's _pair_ resolves for the Host — a Host that answers `blocky_subdomain` while withholding the fleet token passed Preflight and died mid-play on an undefined variable. ADR-0068 requires exactly that withholding of the agent tier's Host, so the trap was waiting for the next one.

The obvious fix is wrong, which is the reason this is written down. Widening the demand to the whole roster asks `publishes_a_name` of a guarded role, and `blocky.meta.yml` declares `subdomain: blocky` — true on every Host regardless of config. `deploy infrastructure` then fails on any Host that serves neither guarded role, which is most of them. The guard and the predicate are not the same question, and a fix that conflates them trades a latent trap for a live break.

Both guard shapes in the repo reduce to the one gate, once the default above is gone. `blocky` and `headscale` are gated on their own `<app>_subdomain` directly. `aoe`, `opencode`, `hermes` and `github_identity` are gated on `group_names`, a Host fact no config answer can evaluate — but of those only `aoe` publishes a name. So the config half decides both classes without anyone evaluating Jinja.

`aoe` is why the precondition is stated rather than assumed. It declared `aoe_subdomain: essaim` in its role defaults, so its guard and its gate genuinely diverged: a Host in group `agent` with no config answer ran the role off the default and died on `aoe_parent_domain`. What kept that from biting was unrelated to any of this — `aoe.meta.yml` and `ruche.meta.yml` are unguarded, so `required_keys_for` demanded the agents pair anyway. That is a coincidence, and it holds only while every playbook rostering the role declares the pair itself.

## Trade-off

- **A guarded role's Zone demand is weaker than an unguarded one's.** An App whose Meta names a subdomain that config never answers is demanded no Zone behind a guard. That is sound only while ansible's guard and the config gate read the same answer, which is what the no-role-default rule buys — and it is a precondition the tree can violate silently, so it is fenced rather than argued.
- **A guarded App's serving gate is now load-bearing twice**: it decides whether the role runs, and whether its Zone is demanded. An operator who withdraws `<app>_subdomain` for a Host turns the App off there, which is the existing ADR-0051 meaning; nothing new is implied, but the key carries more weight than the one-line gate it looks like.

## Alternatives considered

- **Give the Zone demand its own roster walk.** Rejected: it splits the list ADR-0045 keeps single so key demands and Zone demands cannot disagree about what the run is made of. Flagging the entry keeps one walk and makes the divergence explicit at the point each caller reads it.
- **Teach the roster parser to evaluate guards.** Rejected: it cannot. `'agent' in group_names` is an inventory fact, so the parser would answer two guard classes and guess at one. The serving gate answers both without reading the guard at all.
- **Leave Preflight alone; add a role-level `assert`.** Rejected as the primary fix, though kept as the follow-up above. It is local and cheap, and it moves the failure from an undefined-variable traceback to a message naming the Zone and the key — but it still fails mid-play, after the run has begun changing the Host. Preflight exists to refuse before that.
- **Demand every Zone the Host's answered gates imply, run-independently.** Rejected: `get_for_host` falls back to the fleet-wide answer, so every App with a fleet-wide `<app>_subdomain` reads as served on every Host. `ruche` would be refused for withholding the fleet token, which is what ADR-0068 tells it to do.
