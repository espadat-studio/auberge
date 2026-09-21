# ADR-0079: A selector is resolved by a picker, a filter means all

## Status

Accepted, 2026-09-21.

## Decision

A command-line argument that names something the CLI can draw a roster of is one of two things, and the two have opposite rules.

A **Selector** names the thing the command acts on. It is optional, and an omitted one is a question: the handler resolves it through the picker for its noun. `hosts::select_or_arg` is the Host one, `select_account` the Account one, `select_synced_folder` the Folder one. Answering for the operator instead — implying a lone configured Host, bailing with the flag's name — is not the same thing and is what #911 was.

A **Filter** narrows a listing. It is optional, and an omitted one is an answer: every candidate. A Filter may still offer a picker where one can be drawn, with an explicit `[all accounts]` row above the roster, but it must never _require_ the answer a picker would give — a script that passed no filter yesterday reaches everything today.

Everything else is neither, and the third category is named here so it is not mistaken for a gap: an argument supplying a value the command **writes** rather than a subject it reads. `host rename <new>`, `host add --name`, `dns set --ip`. There is no roster, so an omitted one is answered by a clap default or a free-text prompt, never a pick.

### How to tell which one an argument is

The rule the fence cannot infer, so it is written down. Ask what the command acts on when the argument is left off:

| The command then acts on                | The argument is | Omitted means            |
| --------------------------------------- | --------------- | ------------------------ |
| exactly one thing, and which is unclear | a Selector      | ask                      |
| every candidate                         | a Filter        | all                      |
| the same thing either way               | neither         | the default it documents |

Two checks settle the hard cases:

- **Is "all" a coherent answer?** `backup list -H` can list every host's backups, so it is a Filter. `backup verify -H` cannot verify all hosts — a restic repo is verified one host at a time — so it is a Selector. Same flag, same spelling, opposite rule.
- **Can the CLI enumerate the candidates?** `bichon verify-coverage --folder` draws from the account's Synced Folders, so it is a Selector. `ssh keygen --user` is a unix account on the far side of a connection; auberge holds no roster of those, so it is neither, and `default_value = "ansible"` is the whole answer.

Where the two checks disagree with the argument's name, the name loses. `--from-host` on `backup restore` names a Host and there is a roster of those, but omitting it restores from the host being restored onto — one specific implied value, not a question and not "all". It is neither.

### The fence

`tests/a_selector_is_resolved_by_a_picker.rs` holds the rule over the whole clap tree, reading `src/` through `tests/crate_source/mod.rs` (#679). What it derives and what it is told apart:

**Derived**, so a command added tomorrow is covered the day it lands: every `#[derive(Subcommand)]` enum, its variants, and their arguments — following `#[command(flatten)]` into an `Args` struct and into the struct a tuple variant carries, which is where five Subject Nouns live and a scan of the enum bodies alone sees none of them. Also derived: whether an argument can be omitted, and which pickers its variant's match arm can reach.

**Declared**, because intent is not in the syntax: `SUBJECT_NOUNS`, the argument names the crate has a picker for, and `DECLARATIONS`, the 37 occurrences of those names that are not a Selector picked under the argument's own name. Each carries the reason. An omission from that table is read as a Selector, which is the direction that fails loudly — a new filter nobody classified is reported as a selector reaching no picker, and the fix is one line saying so.

The two halves are asserted separately because the five commands that drifted split across them:

| Half                                     | Catches                | Because                                                             |
| ---------------------------------------- | ---------------------- | ------------------------------------------------------------------- |
| every Subject Noun argument is omittable | #922, #923, #924, #925 | `host: String` cannot be left off, so no picker is reachable at all |
| every Selector reaches its own picker    | #911                   | `-H` was already `Option<String>` and resolved without ever asking  |

Per-argument, not per-variant, and `Choice::new("<label>")` is what makes that possible. A variant's handler is one function however many arguments it takes, so "reaches a picker" is satisfied by any one of them; `bichon verify-coverage` has three Selectors and #923 was all three missing at once. The label a picker draws itself under is the only link back to the argument that already exists in the code. Where the argument is named for the command's grammar and the picker for the thing in the roster — `<name>` picking a Host, `backup_id` picking a snapshot — the label is declared beside the reason.

## Why

Five commands drifted off a rule everyone thought was in force, and nothing noticed until someone read the entire command surface by hand:

| Command                    | Was                                    | Issue |
| -------------------------- | -------------------------------------- | ----- |
| `bichon reconcile-folders` | required `-H`                          | #922  |
| `bichon verify-coverage`   | required `-H`, `--account`, `--folder` | #923  |
| `host rename`              | required `<old>`, four siblings prompt | #924  |
| `dns migrate`              | required `--ip`, `set-all` picks one   | #925  |
| `backup verify`            | optional `-H` that refused, not asked  | #911  |

Every one shipped green. The clap surface tests in `main.rs` pin which flags exist and how they are spelled, and a required flag is a perfectly good flag by that standard. Nothing in the tree connected a flag's _name_ to the question of whether omitting it was a question.

The reading that found them was exhaustive and is not repeatable at will. That is the shape this repo fences: a property held by someone having looked once.

### Why the exception list is worth having

A declared-exception list is only as good as its mutation test, and this one's subject is a `#[derive(Subcommand)]` enum rather than a flat token — a scan that matches nothing does not report an empty CLI, it reports a clean one. Three things stop that.

The reach is stated as five exact counts: 13 Subcommand enums, 64 variants, 160 arguments, 66 of them Subject Nouns, 44 of those Selectors. They fail apart rather than together, so a broken enum scan, a broken variant scan and a broken field scan are three different failures. Equality, not a floor, for the reason `crate_source` gives.

Every line of `DECLARATIONS` must name a live argument, so the table cannot outlive what it excuses. A renamed argument arrives unclassified rather than keeping an old excuse.

And the two scanner bugs written while building it are witnesses of their own, because both would have made the fence _loud_ rather than quiet, and a fence that reports twenty-two violations it invented gets deleted:

- Cutting a module at its **first** `#[cfg(test)]` rather than at the trailing test module truncates `src/hosts.rs` at line 220. `select_or_arg` is declared at 459, so every Host picker in the crate disappears and all twenty-two Host Selectors report as unresolved. `crate_source`'s own doc comment warns about exactly this; it was written anyway, which is the argument for the witness rather than the warning.
- `use crate::hosts::{HOST_FLAG, HostManager, select_or_arg as hosts_select_or_arg}` renames the Host picker inside a brace group. `backup create`, `backup sync`, `backup restore` and `sync hermes` all call it under the local name, and a rename scan that only handles `use path::thing as local;` misses every one.

Measured, against the tree as it stands, one regression at a time:

| Regression | Reproduced by                                        | What fires                                                   |
| ---------- | ---------------------------------------------------- | ------------------------------------------------------------ |
| #922       | `-H` back to `String`                                | omittability, naming `BichonCommands::ReconcileFolders.host` |
| #923       | `-H`, `--account`, `--folder` back to `String`       | omittability, naming all three                               |
| #924       | `git apply -R` of its `src/` hunks                   | omittability, naming `HostCommands::Rename.old`              |
| #911       | `resolve_snapshot_host` back to implying a lone Host | picker reach, naming `BackupCommands::Verify.host`           |
| #925       | `git apply -R`                                       | **the counts and the declaration table — not the rule**      |

Three are reproduced by hand rather than reverted: #937 refactored the bichon resolvers and #911's commit also touches `prompt.rs`, so those patches no longer apply in reverse. The shape is what is under test, not the patch.

The last row is the honest one, and it is recorded rather than rounded up. Before #925, `dns migrate` took `ip: String` and had no `--host` at all. `ip` names no roster the CLI can draw — `resolve_ip` prompts for free text — so it is not a Subject Noun and neither half of the rule has anything to say about it. What fires on that revert is the argument count moving and `DnsCommands::Migrate.target.host` going stale in `DECLARATIONS`. That is a real signal naming the exact argument, but the count also moves on any legitimate CLI change, so it is not evidence the rule covers that class.

What the fence holds about `dns migrate` is the shape #925 arrived at: that its target is picked from the Inventory like `set-all`'s, and stays picked. It does not, and cannot, fence the decision to have an inventory-resolved target rather than a bare address — that decision is upstream of any roster the rule can reach.

One more, beyond the five, because it is the claim the per-argument labels rest on: keeping `verify-coverage`'s Host and Account pickers and removing only its Folder one fails naming `BichonCommands::VerifyCoverage.folder`. A per-variant check passes that, and #923 is the reason it matters.

### What it does not claim

It does not check that a picker draws the right roster, or that a Filter's omission truly reaches everything. Both are runtime properties of the resolver, tested where the resolver is. This fence asserts the shape: that the question gets asked at all.

It reaches arguments through two levels of clap indirection and no more. A flatten into a struct that itself flattens further is not followed, and the tree has no such case; an argument arriving that way would be invisible, and the argument count is what would notice.

The obvious import route is shut, which is worth stating because ADR-0046 prefers it. Asking clap itself — `Cli::command()` through `CommandFactory`, which would hand over the argument tree with no parsing at all — needs the `Cli` type, and `Cli` is private in `src/main.rs`. `main.rs` is the binary, and an integration test links the library, so the type is not reachable from `tests/` at any visibility short of moving the whole clap tree into the lib. That move is a real option and a larger change than this fence; until someone makes it, "which arguments a derive macro turns into flags" is a question about source as text, which is the clause of ADR-0046 that applies.

Delimiters are counted, not lexed. An unbalanced `{`, `(` or `[` inside a `help = "..."` string would mis-nest the scan; the tree has none, the argument count is what proves it, and the failure is a panic naming the byte rather than a span quietly read wrong.

`SUBJECT_NOUNS` is a vocabulary, not a type. An argument named for a roster the crate cannot draw — `--tags`, `--group` — is outside the rule, because "resolve it through its picker" has no referent. That is a real hole: a future `--group` with a group picker behind it is unfenced until the noun is listed. Listing it is one line, and the count moving is what prompts it.
