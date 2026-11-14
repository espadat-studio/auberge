# ADR-0089: A run resolves only the `!` values it reads

## Status

Accepted, 2026-11-14. **Amends #1240**, which made a run resolve every `!` value in `config.toml`, optional keys included.

**Amended by [ADR-0090](./0090-a-run-resolves-only-the-zone-tokens-it-reads.md)** (2026-11-14): Zone tokens follow the Read Set too.

## Decision

A run resolves a `!` value only when its key is in the run's **Read Set**. A `!` value outside it is never run and never reaches the run's extra-vars.

- **Only `!` values are filtered.** Plain values, `!!` escapes included, reach every run as before. Plain-text secrets therefore still reach every run.
- **One walk.** `services::read_set` computes the Read Set. `tests/variable_answerability.rs` reads runs, reachable roles and names through the same module, so the fence and the runtime cannot disagree about which files a run reaches.
- **The walk.** It starts from the roles `selected_roles` picks for `(playbook, tags)`, guarded ones included. It follows `meta/main.yml` dependencies and `include_role` to a fixpoint. It reads every name, guarded or not, in the playbook, in each reached role's templated YAML and `templates/`, in `group_vars/` and in `inventory.yml`, which every run loads.
- **Tags narrow the Read Set.** `-t <role>` reads only what that role reaches, plus roster roles tagged `always`.
- **Computed variable access means the run reads every key.** `lookup`/`query`/`q` over `vars`, `ansible.builtin.vars` or `varnames`, `vars[expr]`, `hostvars[...][expr]` and an `include_role` whose name is templated all make the Read Set everything. A literal name (`lookup('vars', 'x')`, `vars['x']`, `vars.x`) counts as a read of `x`. A free-form `include_role: name=…` also reads every key. Today only blocky does this, so `infrastructure.yml` resolves every value. The other 15 playbooks narrow.
- **Fail fast.** A file the walk cannot read or parse, a role it cannot find, or a missing `group_vars/` or `inventory.yml` fails the run. It is never skipped.
- **`[hosts.<name>]` overrides** are filtered by the same Read Set.
- **Rust code reads config through `Config`,** never `Preflight::flat_vars`, which now serves Ansible only.

## Why

#1240 resolved every `!` value on every run, and #1321 made each command run once per invocation. With a hardware-backed password manager (a YubiKey PIN and touch, a TPM unseal), a single-app deploy still asked for every secret in `config.toml`. It also put every secret into every run's extra-vars, so a run held secrets it never reads. Resolving only the Read Set fixes both. A run gets the `!` secrets it reads and no others.

The cost is that a broken command on a key no run reads is no longer caught early. A broken command is still caught by the first run that reads it, before Ansible starts.

## Consequences

- A broken `!` command on a key the run never reads no longer fails the run. It fails the first run that reads it.
- A `!` failure on a key inside the Read Set still stops the run before Ansible starts, naming the key.
- **Zone tokens still reach every run.** Computed Vars resolve `host_zones` over every Meta (ADR-0081, ADR-0082). Narrowing them is #1323.
- A name the walk misses is a secret the run never gets. `tests/read_set_witness.rs` guards that independently: every Key Registry name that appears as a whole word in a file the run renders must be in its Read Set. It reaches roles by regex over raw text, not through the walk.

## Rejected

- **Whole-word matching as the runtime walk.** It is the witness's method. As the authority it would treat comments and literals as reads, and it cannot tell a computed lookup from a literal one.
- **Filtering plain values too.** They cost nothing to resolve, and filtering them would change what roles see for no security gain on this axis.
- **Leaving computed access out.** blocky builds variable names at run time; a Read Set that ignored that would hand blocky empty strings.
