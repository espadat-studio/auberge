# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root — the project's domain glossary.
- **`meta/adr.md`** — the curated overview of foundational architectural decisions (native systemd by default, Rust CLI, Ansible, etc.).
- **`meta/adr/`** — granular per-decision ADR files added over time. Read the ones touching the area you're about to work in.

> ADRs live under `meta/`, not the canonical `docs/adr/`. `docs/` is the Starlight project that builds auberge.espadat.com: a page added under `docs/src/content/docs/` is published, and so is anything in `docs/public/`. Internal material — ADRs, throwaway plans under `meta/agents/plans/`, research — stays in `meta/`.

If any of these files don't exist, **proceed silently**. Don't flag their absence; don't suggest creating them upfront. The producer skill (`/grill-with-docs`) creates them lazily when terms or decisions actually get resolved.

## File structure

Single-context repo:

```
/
├── CONTEXT.md
├── meta/
│   ├── adr.md            ← curated overview of foundational decisions
│   ├── adr/              ← granular per-decision ADRs (created lazily)
│   │   └── 0001-…md
│   ├── agents/           ← agent skill config (this folder)
│   │   └── plans/        ← throwaway plans, deleted once the work lands
│   └── roadmap.md
├── docs/                 ← published Starlight site (a build directory, not for ADRs)
└── src/
```

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal — either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/grill-with-docs`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (xyz) — but worth reopening because…_
