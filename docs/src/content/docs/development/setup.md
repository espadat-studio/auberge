---
title: "Development Setup"
---

## Quick Start

```bash
./setup              # Install tools (mise, Rust, Ansible)
cargo build          # Build
cargo test           # Test
```

## Tools Installed

- mise - Environment manager
- Rust toolchain
- Ansible and ansible-lint
- dprint, dasel, pkl

## Documentation site

`docs/` is an Astro Starlight project, not loose markdown.

```bash
cd docs
npm install
npm run dev              # live reload, no search index
npm run build            # static output in docs/dist
npm run preview          # serve the build
```

Pagefind indexes at build time, so search only works under `preview`, never `dev`.

## Commands

```bash
cargo build --release    # Build release binary
cargo test               # Run tests
cargo clippy            # Lint
ansible-lint            # Lint playbooks
```
