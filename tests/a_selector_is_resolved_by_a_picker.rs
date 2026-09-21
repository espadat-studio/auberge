//! ADR-0079's rule, held over the whole command surface: an argument naming
//! the thing a command acts on is optional and resolved by a picker; an
//! argument that narrows a listing is optional and means "all" when omitted.
//!
//! Five commands drifted off that rule and nothing noticed.
//! `bichon reconcile-folders` and `verify-coverage` took bare required names
//! (#922, #923), `host rename` took a required old name while its four
//! siblings prompt (#924), `dns migrate` took a required address while
//! `set-all` picks one (#925), and `backup verify` refused rather than asked
//! (#911). Every one was found by reading the whole surface by hand, once.
//!
//! The two halves are asserted separately because the five split across them.
//! Four were a type: `host: String` cannot be omitted, so no picker is
//! reachable however the handler is written. The fifth was not — `backup
//! verify -H` was already `Option<String>` and resolved through
//! `sole_configured_host`, which refuses where a picker would ask. A fence
//! over the type alone passes #911; a fence over the handler alone passes the
//! other four, because a required argument's handler still reaches a picker
//! for the arguments beside it.
//!
//! ## What is derived and what is declared
//!
//! Derived, so a new command is covered the day it lands: every
//! `#[derive(Subcommand)]` enum, its variants, and their arguments — through
//! `#[command(flatten)]` into an `Args` struct, and into the struct a tuple
//! variant carries. Also derived: whether an argument is omittable, and which
//! pickers its variant's handler can reach.
//!
//! Declared, because no scan can infer intent: [`SUBJECT_NOUNS`], the argument
//! names that denote something with a roster, and [`DECLARATIONS`], the
//! occurrences of those names that are not plain Selectors. `backup list -H`
//! and `backup verify -H` are the same spelling with opposite meanings — one
//! narrows a listing, one names the subject — and only a person can say which.
//!
//! ## Why it cannot pass vacuously
//!
//! The subject here is a `#[derive(Subcommand)]` enum rather than a flat
//! token, so a scan that matches nothing reports a clean CLI. Three things
//! stop that. The reach is stated as five exact counts, so a parser that stops
//! seeing a shape fails instead of shrinking. Every line of [`DECLARATIONS`]
//! must name a live argument, so the table cannot outlive what it excuses.
//! And the two parsing traps this fence fell into while being written are
//! witnesses of their own: `use ... as ...` inside a brace group, which hides
//! `hosts_select_or_arg` and makes four correct commands look unresolved, and
//! cutting a module at its *first* `#[cfg(test)]` rather than at the trailing
//! test module, which hides `src/hosts.rs` from line 220 on — `select_or_arg`
//! is declared at 459, so every Host picker in the crate disappears and the
//! whole fence goes quiet. `crate_source`'s own doc comment warns about the
//! second; it was written anyway.
//!
//! Reads `src/` through `crate_source` rather than walking it again, so the
//! reach pin inside `modules` is inherited (#679). Per ADR-0046 this is a
//! question about source as text — which arguments a `derive` macro turns into
//! flags, and which functions a handler can reach — so it is asked here rather
//! than by importing an item.

mod crate_source;

use crate_source::{Module, modules};
use std::collections::{BTreeMap, BTreeSet};

/// Argument names that denote something the crate can draw a roster of.
///
/// The gate that makes the rule checkable at all: `--before`, `--max-age` and
/// `--dry-run` name no roster, so they are not selectors, not filters, and not
/// this fence's business. An argument named here is one of the two — or is
/// excused by name in [`DECLARATIONS`].
///
/// Plurals are listed beside their singulars (`app`/`apps`,
/// `playbook`/`playbooks`) because clap spells a repeated flag that way and
/// the plural is where "omitted means all" usually lives.
///
/// Held live by [`every_subject_noun_names_a_live_argument`]. A name no
/// argument carries excuses nothing and fences nothing, and two got in here
/// before that test existed: `node`, which is the *picker's* label where the
/// argument is `name`, and `target`, which is only ever the flatten
/// `TargetAddressArg` and so is recursed through rather than reached as a
/// leaf. Deleting either changed no other assertion.
const SUBJECT_NOUNS: &[&str] = &[
    "account",
    "app",
    "apps",
    "backup_id",
    "folder",
    "from_host",
    "host",
    "key",
    "name",
    "old",
    "playbook",
    "playbooks",
    "subdomain",
    "user",
];

/// What an occurrence of a [`SUBJECT_NOUNS`] name is, when it is not a
/// Selector picked under its own name.
enum Role {
    /// Narrows a listing. Omitted means every candidate, so no picker is
    /// required — and the argument must still be omittable, or "all" is
    /// unreachable.
    Filter,
    /// Omitted is answered without a picker, by the value named here: a clap
    /// default, a free-text prompt, or another argument's answer. The
    /// argument supplies something the command writes, or narrows nothing.
    Implied(&'static str),
    /// Still a Selector. Its picker is labelled something other than the
    /// argument's name, because the argument is named for the command's
    /// grammar and the picker for the thing in the roster.
    PickedAs(&'static str),
}

/// One occurrence of a Subject Noun that is not a plain Selector.
struct Declaration {
    /// `BackupCommands::Verify.app`, or `DnsCommands::Migrate.target.host`
    /// through a flatten.
    at: &'static str,
    role: Role,
    why: &'static str,
}

/// Every occurrence of a [`SUBJECT_NOUNS`] name that is not a Selector picked
/// under its own name, with the reason it is not.
///
/// An omission here is a Selector, which is the direction that fails loudly: a
/// new filter nobody classified is reported as a selector reaching no picker.
/// The reverse — a selector wrongly excused — takes a deliberate line with a
/// reason someone has to write, which is the whole point of the table.
const DECLARATIONS: &[Declaration] = &[
    // --- Filters: omitted means every candidate ---
    Declaration {
        at: "BackupCommands::List.host",
        role: Role::Filter,
        why: "listing every host's backups is the default view; `backup verify -H` is the \
              same spelling naming a subject, because a restic repo is verified one host at a time",
    },
    Declaration {
        at: "BackupCommands::List.app",
        role: Role::Filter,
        why: "narrows the listing to one app; omitted lists them all",
    },
    Declaration {
        at: "BackupCommands::Verify.app",
        role: Role::Filter,
        why: "narrows which apps the snapshot age is checked for; omitted checks every app \
              in the snapshot",
    },
    Declaration {
        at: "BackupCommands::Create.apps",
        role: Role::Filter,
        why: "narrows the Backup Recipe roster; omitted backs up every app the host declares",
    },
    Declaration {
        at: "BackupCommands::Sync.apps",
        role: Role::Filter,
        why: "narrows the Backup Recipe roster; omitted syncs every app the host declares",
    },
    Declaration {
        at: "BackupCommands::Restore.apps",
        role: Role::Filter,
        why: "narrows what is restored out of the snapshot; omitted opens the app picker \
              over everything the snapshot holds",
    },
    Declaration {
        at: "BackupCommands::Push.host",
        role: Role::Filter,
        why: "narrows the local backup roster before `resolve_backup_dir` picks one; the \
              subject `push` acts on is the backup, not the host",
    },
    Declaration {
        at: "BichonCommands::ReconcileFolders.account",
        role: Role::Filter,
        why: "narrows which accounts are reconciled; omitted reconciles all, and the picker \
              offers `[all accounts]` above the roster (#922)",
    },
    Declaration {
        at: "BichonCommands::Rescan.account",
        role: Role::Filter,
        why: "narrows which accounts are rescanned; omitted rescans all",
    },
    Declaration {
        at: "DnsCommands::List.subdomain",
        role: Role::Filter,
        why: "narrows the record listing; omitted lists every record",
    },
    Declaration {
        at: "ConfigCommands::Init.playbooks",
        role: Role::Filter,
        why: "narrows the scaffold to those playbooks' required keys; omitted emits the \
              whole Key Registry",
    },
    Declaration {
        at: "Commands::Deploy.apps",
        role: Role::Filter,
        why: "the deploy roster, narrowed; omitted opens the deploy menu, and `--all` is \
              the explicit everything (ADR-0077)",
    },
    // --- Implied: omitted is answered without a picker ---
    Declaration {
        at: "AnsibleCommands::Run.user",
        role: Role::Implied("the Host's inventory user"),
        why: "a unix account to connect as, not a roster the CLI can enumerate",
    },
    Declaration {
        at: "AnsibleCommands::Bootstrap.user",
        role: Role::Implied("the Host's inventory user"),
        why: "a unix account on a machine not yet in the roster; there is nothing to pick from",
    },
    Declaration {
        at: "HostCommands::Add.user",
        role: Role::Implied("a free-text prompt in the add wizard"),
        why: "the new Host's ssh user, typed rather than chosen",
    },
    Declaration {
        at: "SshCommands::Keygen.user",
        role: Role::Implied("the literal `ansible`"),
        why: "a unix account on the remote, defaulted by clap",
    },
    Declaration {
        at: "SshCommands::AddKey.user",
        role: Role::Implied("the literal `ansible`"),
        why: "a unix account on the remote, defaulted by clap",
    },
    Declaration {
        at: "OpmlCommands::ExportOpml.user",
        role: Role::Implied("the literal `admin`"),
        why: "a FreshRSS account name, defaulted by clap; auberge holds no FreshRSS roster",
    },
    Declaration {
        at: "OpmlCommands::ImportOpml.user",
        role: Role::Implied("the literal `admin`"),
        why: "a FreshRSS account name, defaulted by clap; auberge holds no FreshRSS roster",
    },
    Declaration {
        at: "HostCommands::Add.name",
        role: Role::Implied("a free-text prompt in the add wizard"),
        why: "names the Host being created; there is no roster it is drawn from",
    },
    Declaration {
        at: "HeadscaleCommands::AddUser.name",
        role: Role::Implied("a free-text prompt"),
        why: "names the headscale user being created; `remove-user --name` is the same \
              spelling picking an existing one",
    },
    Declaration {
        at: "BackupCommands::Restore.from_host",
        role: Role::Implied("the --host being restored onto"),
        why: "names the host whose snapshot to read; omitted is a same-host restore, which \
              is the common case, so the absence is an answer rather than a question",
    },
    // --- PickedAs: a Selector whose picker is labelled differently ---
    Declaration {
        at: "BackupCommands::Restore.backup_id",
        role: Role::PickedAs("backup"),
        why: "the argument is named for the id a script passes; the picker draws snapshots",
    },
    Declaration {
        at: "BackupCommands::Push.backup_id",
        role: Role::PickedAs("host backup"),
        why: "picked out of the local roster already narrowed by -H, so the rows are one \
              host's backups",
    },
    Declaration {
        at: "BichonCommands::VerifyCoverage.folder",
        role: Role::PickedAs("synced folder"),
        why: "only a folder the account still syncs can have its coverage vouched for, so \
              the picker says which set it draws from (#923)",
    },
    Declaration {
        at: "ConfigCommands::Set.key",
        role: Role::PickedAs("config key"),
        why: "`key` is bare in the grammar; the roster is the Key Registry",
    },
    Declaration {
        at: "ConfigCommands::Get.key",
        role: Role::PickedAs("config key"),
        why: "`key` is bare in the grammar; the roster is the Key Registry",
    },
    Declaration {
        at: "ConfigCommands::Remove.key",
        role: Role::PickedAs("config key"),
        why: "`key` is bare in the grammar; the roster is the Key Registry",
    },
    Declaration {
        at: "HostCommands::Remove.name",
        role: Role::PickedAs("host"),
        why: "the positional is `<name>`; the roster is the Inventory",
    },
    Declaration {
        at: "HostCommands::Show.name",
        role: Role::PickedAs("host"),
        why: "the positional is `<name>`; the roster is the Inventory",
    },
    Declaration {
        at: "HostCommands::Edit.name",
        role: Role::PickedAs("host"),
        why: "the positional is `<name>`; the roster is the Inventory",
    },
    Declaration {
        at: "HostCommands::DetectTailscaleIp.name",
        role: Role::PickedAs("host"),
        why: "the positional is `<name>`; the roster is the Inventory",
    },
    Declaration {
        at: "HostCommands::Rename.old",
        role: Role::PickedAs("host"),
        why: "the current name, picked out of the Inventory; `new` beside it is free text \
              and is not a Subject Noun (#924)",
    },
    Declaration {
        at: "HeadscaleCommands::TagNode.name",
        role: Role::PickedAs("node"),
        why: "a machine already enrolled in the tailnet, drawn from headscale's own roster",
    },
    Declaration {
        at: "HeadscaleCommands::RemoveUser.name",
        role: Role::PickedAs("user"),
        why: "an existing headscale user; `add-user --name` is the same spelling naming a \
              new one",
    },
    Declaration {
        at: "DnsCommands::Migrate.target.host",
        role: Role::PickedAs("host"),
        why: "the flattened `TargetAddressArg` names the address by the Host that has it (#925)",
    },
    Declaration {
        at: "DnsCommands::SetAll.target.host",
        role: Role::PickedAs("host"),
        why: "the flattened `TargetAddressArg` names the address by the Host that has it",
    },
];

// ---------------------------------------------------------------------------
// Reading the crate
// ---------------------------------------------------------------------------

/// `source` up to its trailing `#[cfg(test)]\nmod tests`, or all of it.
///
/// The cut that matters, and the one this fence got wrong first: cutting at
/// the *first* `#[cfg(test)]` truncates `src/hosts.rs` at line 220, and
/// `select_or_arg` — the Host picker every `-H` in the crate resolves through
/// — is declared at 459. Every Host Selector then reads as reaching no picker,
/// which is a fence reporting twenty-two violations it invented.
fn without_test_module(source: &str) -> &str {
    source
        .find("\n#[cfg(test)]\nmod tests")
        .map_or(source, |at| &source[..at])
}

/// `source` with whole-line `//` comments blanked, positions preserved.
///
/// Blanked rather than removed so every byte offset in the original still
/// addresses the same character — the scanners below slice by index. Prose
/// must not be read as code here for the usual reason and one extra: this
/// fence's own vocabulary (`select_item`, `Choice::new("host")`) is quoted
/// throughout the crate's doc comments, so an uncut scan finds a picker in
/// every module that merely talks about one.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                " ".repeat(line.len())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The index of the delimiter closing the one at `open_at`.
///
/// Counts delimiters without knowing about string literals, so an unbalanced
/// `{`, `(` or `[` inside a `help = "..."` would mis-nest. The tree has none —
/// the argument counts below are what proves it — and the failure is a panic
/// naming the byte rather than a scan that quietly reads the wrong span. A
/// Rust lexer to rule it out is a parser nobody signed up to maintain
/// (ADR-0046); if this ever fires, balance the literal or brace it in a
/// `r#".."#`.
fn closes(source: &str, open_at: usize, open: u8, close: u8) -> usize {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open_at) {
        if *byte == open {
            depth += 1;
        } else if *byte == close {
            depth -= 1;
            if depth == 0 {
                return index;
            }
        }
    }
    panic!(
        "unbalanced {} from byte {open_at} — most likely an unbalanced {} inside a string \
         literal, which this scanner does not lex",
        open as char, open as char
    );
}

/// One field of a struct or struct-variant body: its attributes, name and type.
struct Field {
    attributes: String,
    name: String,
    ty: String,
}

/// The fields of a braced body, with each field's preceding `#[...]` blocks
/// attached.
///
/// Hand-scanned rather than regexed because the type is the thing being
/// asserted about and a line-oriented match reads `Option<Vec<String>>` and
/// `HashMap<String, bool>` the same way — the second has a comma inside its
/// generics, which is exactly where a naive split lands.
fn fields(body: &str) -> Vec<Field> {
    let bytes = body.as_bytes();
    let mut found = Vec::new();
    let mut attributes = String::new();
    let mut index = 0usize;

    while index < body.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if body[index..].starts_with("//") {
            index = body[index..]
                .find('\n')
                .map_or(body.len(), |newline| index + newline);
            continue;
        }
        if body[index..].starts_with("#[") {
            let end = closes(body, index + 1, b'[', b']');
            attributes.push_str(&body[index..=end]);
            index = end + 1;
            continue;
        }

        let start = index;
        let mut depth = 0i32;
        while index < body.len() {
            match bytes[index] {
                b'<' | b'(' | b'[' | b'{' => depth += 1,
                b'>' | b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => break,
                _ => {}
            }
            index += 1;
        }
        let declaration = body[start..index.min(body.len())].trim();
        index += 1;

        if let Some((name, ty)) = declaration.split_once(':') {
            found.push(Field {
                attributes: std::mem::take(&mut attributes),
                name: name.trim().trim_start_matches("pub ").trim().to_string(),
                ty: ty.trim().to_string(),
            });
        } else {
            attributes.clear();
        }
    }

    found
}

/// Every `#[derive(Subcommand)]` enum in the crate, as name -> body.
fn subcommand_enums(walked: &[Module]) -> BTreeMap<String, String> {
    declared_blocks(walked, "Subcommand", "enum")
}

/// Every clap `Args` struct in the crate, as name -> body.
///
/// Needed because a variant reaches arguments two ways that are invisible to a
/// scan of the enum alone: `#[command(flatten)] target: TargetAddressArg`
/// puts `--host` and `--ip` on `dns migrate`, and the tuple variant
/// `Deploy(DeployCmd)` puts `--host` and the `apps` roster on `deploy`. Five
/// Subject Nouns live only behind those two shapes, and a scan of the enum
/// bodies alone sees none of them.
fn args_structs(walked: &[Module]) -> BTreeMap<String, String> {
    declared_blocks(walked, "Args", "struct")
}

/// Bodies of every `item` whose derive list names `trait_name`.
///
/// One function for both because the only difference is the keyword: the
/// derive spellings in this crate are `#[derive(Subcommand)]`,
/// `#[derive(Args)]` and `#[derive(clap::Args)]`, so the derive list is
/// matched on the bare trait name as its own token rather than on the
/// attribute text.
fn declared_blocks(walked: &[Module], trait_name: &str, item: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();

    for module in walked {
        let source = code_only(without_test_module(&module.source));
        let mut from = 0usize;
        while let Some(at) = source[from..].find("#[derive(") {
            let open = from + at + "#[derive".len();
            let end = closes(&source, open, b'(', b')');
            let derives = &source[open + 1..end];
            from = end + 1;

            let names = derives
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|token| token == trait_name);
            if !names {
                continue;
            }

            let rest = &source[from..];
            let keyword = format!("{item} ");
            let Some(keyword_at) = rest.find(&keyword) else {
                continue;
            };
            // Only the item this derive is attached to: anything but
            // whitespace, `pub` and further attributes in between means the
            // derive belongs to something else.
            let between = &rest[..keyword_at];
            if !between
                .replace("pub", " ")
                .chars()
                .all(|c| c.is_whitespace() || c == '#' || c == '[' || c == ']')
            {
                continue;
            }

            let after = keyword_at + keyword.len();
            let name: String = rest[after..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let Some(brace) = rest[after..].find('{') else {
                continue;
            };
            let open_brace = from + after + brace;
            let close_brace = closes(&source, open_brace, b'{', b'}');
            found.insert(name, source[open_brace + 1..close_brace].to_string());
        }
    }

    found
}

/// One command-line argument, addressed the way [`DECLARATIONS`] spells it.
struct Argument {
    /// `BackupCommands::Verify.app`, or `DnsCommands::Migrate.target.host`
    /// when it arrives through a flatten.
    at: String,
    /// The leaf name, which is what [`SUBJECT_NOUNS`] is matched against.
    name: String,
    ty: String,
    attributes: String,
}

impl Argument {
    /// `BackupCommands::Verify` — the key a picker reach is computed per.
    ///
    /// Derived rather than carried beside [`Argument::at`]: the two were the
    /// same fact spelled twice, and the pair had to be threaded through the
    /// flatten recursion to keep them agreeing.
    fn variant(&self) -> &str {
        self.at.split('.').next().unwrap_or(&self.at)
    }

    /// `true` when the argument can be left off the command line.
    ///
    /// A `bool` flag is omittable by construction and never a Subject Noun, so
    /// it is not special-cased here. `Vec<_>` is clap's repeated flag, empty
    /// when absent; a `default_value` answers for a bare type.
    fn omittable(&self) -> bool {
        self.ty.starts_with("Option<")
            || self.ty.starts_with("Vec<")
            || self.attributes.contains("default_value")
    }
}

/// A variant's shape, which decides where its arguments live.
enum Shape {
    /// `Prune { dry_run: bool }` — the arguments are the fields.
    Braced(String),
    /// `Deploy(DeployCmd)` — the arguments are the named type's fields, if it
    /// is an `Args` struct; if it is another `Subcommand` enum they are walked
    /// under that enum instead.
    Wrapping(String),
    /// `Playbook` — no arguments.
    Bare,
}

/// The variants of an enum body, in declaration order.
fn variants(body: &str) -> Vec<(String, Shape)> {
    let bytes = body.as_bytes();
    let mut found = Vec::new();
    let mut index = 0usize;

    while index < body.len() {
        if bytes[index].is_ascii_whitespace() || bytes[index] == b',' {
            index += 1;
            continue;
        }
        if body[index..].starts_with("//") {
            index = body[index..]
                .find('\n')
                .map_or(body.len(), |newline| index + newline);
            continue;
        }
        if body[index..].starts_with("#[") {
            index = closes(body, index + 1, b'[', b']') + 1;
            continue;
        }

        let name: String = body[index..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        index += name.len();
        while index < body.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        let shape = match bytes.get(index) {
            Some(b'{') => {
                let end = closes(body, index, b'{', b'}');
                let inner = body[index + 1..end].to_string();
                index = end + 1;
                Shape::Braced(inner)
            }
            Some(b'(') => {
                let end = closes(body, index, b'(', b')');
                let inner = body[index + 1..end].trim().to_string();
                index = end + 1;
                Shape::Wrapping(inner)
            }
            _ => Shape::Bare,
        };
        found.push((name, shape));
    }

    found
}

/// Every argument the clap tree declares, addressed as [`DECLARATIONS`]
/// spells it.
fn arguments(walked: &[Module]) -> Vec<Argument> {
    let enums = subcommand_enums(walked);
    let structs = args_structs(walked);
    let mut found = Vec::new();

    for (enum_name, body) in &enums {
        for (variant, shape) in variants(body) {
            let at = format!("{enum_name}::{variant}");
            let body = match shape {
                Shape::Braced(inner) => inner,
                // A nested Subcommand enum is walked as itself.
                Shape::Wrapping(ref ty) if enums.contains_key(ty) => continue,
                Shape::Wrapping(ty) => match structs.get(&ty) {
                    Some(inner) => inner.clone(),
                    None => continue,
                },
                Shape::Bare => continue,
            };
            collect_arguments(&body, &at, &structs, &mut found);
        }
    }

    found
}

/// The fields of `body` as arguments under `path`, following a flatten into
/// the struct it names.
fn collect_arguments(
    body: &str,
    path: &str,
    structs: &BTreeMap<String, String>,
    found: &mut Vec<Argument>,
) {
    for field in fields(body) {
        let at = format!("{path}.{}", field.name);
        if field.attributes.contains("#[command(flatten)]")
            && let Some(inner) = structs.get(&field.ty)
        {
            collect_arguments(inner, &at, structs, found);
            continue;
        }
        found.push(Argument {
            at,
            name: field.name,
            ty: field.ty,
            attributes: field.attributes,
        });
    }
}

// ---------------------------------------------------------------------------
// Which pickers a handler can reach
// ---------------------------------------------------------------------------

/// The crate's functions and who calls whom, by name.
struct Calls {
    bodies: BTreeMap<String, Vec<String>>,
    edges: BTreeMap<String, BTreeSet<String>>,
}

/// `use` renames, as local name -> original.
///
/// Load-bearing, and the second trap this fence fell into: `use
/// crate::hosts::{HOST_FLAG, HostManager, select_or_arg as
/// hosts_select_or_arg}` renames the Host picker inside a brace group, and
/// `backup create`, `backup sync`, `backup restore` and `sync hermes` all
/// call it under the local name. Missing the rename makes four correct
/// commands report as reaching no picker — a fence loud about the wrong
/// thing, which gets a fence deleted.
fn renames(source: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for statement in source.split(';') {
        let Some(at) = statement.find("use ") else {
            continue;
        };
        let mut rest = &statement[at..];
        while let Some(as_at) = rest.find(" as ") {
            let original: String = rest[..as_at]
                .chars()
                .rev()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let local: String = rest[as_at + 4..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !original.is_empty() && !local.is_empty() {
                found.push((local, original));
            }
            rest = &rest[as_at + 4..];
        }
    }
    found
}

/// Every name called as a function in `body`, renames resolved.
fn called(body: &str, known: &BTreeSet<String>, aliases: &BTreeMap<String, String>) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut found = Vec::new();
    let mut start = None;

    for (index, byte) in bytes.iter().enumerate() {
        let word = byte.is_ascii_alphanumeric() || *byte == b'_';
        match (word, start) {
            (true, None) => start = Some(index),
            (false, Some(from)) => {
                if *byte == b'(' {
                    let token = &body[from..index];
                    let name = aliases.get(token).map_or(token, String::as_str);
                    if known.contains(name) {
                        found.push(name.to_string());
                    }
                }
                start = None;
            }
            _ => {}
        }
    }

    found
}

fn call_graph(walked: &[Module]) -> Calls {
    let sources: Vec<String> = walked
        .iter()
        .map(|module| code_only(without_test_module(&module.source)))
        .collect();

    let mut aliases = BTreeMap::new();
    let mut bodies: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for source in &sources {
        for (local, original) in renames(source) {
            aliases.insert(local, original);
        }
        for (name, body) in definitions(source) {
            bodies.entry(name).or_default().push(body);
        }
    }

    let known: BTreeSet<String> = bodies.keys().cloned().collect();
    let edges = bodies
        .iter()
        .map(|(name, all)| {
            let out: BTreeSet<String> = all
                .iter()
                .flat_map(|body| called(body, &known, &aliases))
                .collect();
            (name.clone(), out)
        })
        .collect();

    Calls { bodies, edges }
}

/// Every `fn` defined in `source`, as name -> body.
///
/// Bodies only: a trait method signature ending in `;` has none, and a
/// closure's body is part of the function it sits in, which is what makes
/// `signal::with_ctrlc(|| run_backup_verify(..))` in the dispatch reach
/// `run_backup_verify`.
fn definitions(source: &str) -> Vec<(String, String)> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut from = 0usize;

    while let Some(at) = source[from..].find("fn ") {
        let start = from + at;
        from = start + 3;
        let preceded = start
            .checked_sub(1)
            .is_none_or(|before| !(bytes[before].is_ascii_alphanumeric() || bytes[before] == b'_'));
        if !preceded {
            continue;
        }

        let name: String = source[start + 3..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }

        let Some(paren) = source[start..].find('(').map(|at| start + at) else {
            continue;
        };
        let params = closes(source, paren, b'(', b')');
        let Some(next) = source[params..].find(['{', ';']).map(|at| params + at) else {
            continue;
        };
        if bytes[next] == b';' {
            continue;
        }
        let end = closes(source, next, b'{', b'}');
        found.push((name, source[next..=end].to_string()));
        from = end;
    }

    found
}

impl Calls {
    /// Every function reachable from `start`, `start` included.
    fn reach(&self, start: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::from([start.to_string()]);
        let mut pending = vec![start.to_string()];
        while let Some(current) = pending.pop() {
            for next in self.edges.get(&current).into_iter().flatten() {
                if seen.insert(next.clone()) {
                    pending.push(next.clone());
                }
            }
        }
        seen
    }

    /// `true` when any function reachable from `from` constructs a picker
    /// labelled `label`.
    ///
    /// `Choice::new(..)` is the label the picker draws itself under, and it is
    /// the only per-argument link available: a variant's handler is one
    /// function whatever its arguments, so reaching "a picker" says nothing
    /// about *which* of three subjects got asked for. `verify-coverage` is the
    /// case in point — host, account and folder, three pickers, and #923 was
    /// all three missing at once.
    fn reaches_picker(&self, from: &BTreeSet<String>, label: &str) -> bool {
        let needle = format!("Choice::new(\"{label}\")");
        from.iter()
            .filter_map(|name| self.bodies.get(name))
            .flatten()
            .any(|body| body.contains(&needle))
    }
}

/// Every function a variant's match arm can reach, over every arm matching it.
fn handlers(walked: &[Module], calls: &Calls) -> BTreeMap<String, BTreeSet<String>> {
    let known: BTreeSet<String> = calls.bodies.keys().cloned().collect();
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for module in walked {
        let source = code_only(without_test_module(&module.source));
        let aliases: BTreeMap<String, String> = renames(&source).into_iter().collect();
        for (at, body) in arms(&source) {
            let reached = found.entry(at).or_default();
            for callee in called(&body, &known, &aliases) {
                reached.extend(calls.reach(&callee));
            }
        }
    }

    found
}

/// Every `<Enum>::<Variant> <pattern> => <expression>` in `source`, as
/// `Enum::Variant -> expression`.
///
/// A construction (`let _push = BackupCommands::Push { .. };`) carries no
/// `=>` and is skipped, which is what keeps `main.rs`'s clap-surface pins out
/// of the graph.
fn arms(source: &str) -> Vec<(String, String)> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut from = 0usize;

    while let Some(at) = source[from..].find("::") {
        let colons = from + at;
        from = colons + 2;

        let before: String = source[..colons]
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if !before.ends_with("Commands") {
            continue;
        }
        let variant: String = source[colons + 2..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if variant.is_empty() || !variant.starts_with(|c: char| c.is_uppercase()) {
            continue;
        }

        let mut index = colons + 2 + variant.len();
        while index < source.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        match bytes.get(index) {
            Some(b'{') => index = closes(source, index, b'{', b'}') + 1,
            Some(b'(') => index = closes(source, index, b'(', b')') + 1,
            _ => {}
        }
        while index < source.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if source[index..].starts_with("if ") {
            index += source[index..].find("=>").unwrap_or(0);
        }
        if !source[index..].starts_with("=>") {
            continue;
        }

        index += 2;
        let start = index;
        let mut depth = 0i32;
        while index < source.len() {
            match bytes[index] {
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                b',' if depth == 0 => break,
                _ => {}
            }
            index += 1;
        }
        found.push((
            format!("{before}::{variant}"),
            source[start..index].to_string(),
        ));
    }

    found
}

// ---------------------------------------------------------------------------
// The rule
// ---------------------------------------------------------------------------

fn declaration(at: &str) -> Option<&'static Declaration> {
    DECLARATIONS.iter().find(|entry| entry.at == at)
}

/// Arguments whose name is a Subject Noun, which is the domain the rule
/// reaches.
fn candidates(walked: &[Module]) -> Vec<Argument> {
    arguments(walked)
        .into_iter()
        .filter(|argument| SUBJECT_NOUNS.contains(&argument.name.as_str()))
        .collect()
}

/// The picker label a Selector must reach, or `None` when the argument is
/// declared as something other than a Selector.
fn selector_label(argument: &Argument) -> Option<String> {
    match declaration(&argument.at).map(|entry| &entry.role) {
        None => Some(argument.name.clone()),
        Some(Role::PickedAs(label)) => Some((*label).to_string()),
        Some(Role::Filter | Role::Implied(_)) => None,
    }
}

#[test]
fn every_subject_noun_argument_can_be_omitted() {
    let walked = modules();
    let offenders: Vec<String> = candidates(&walked)
        .iter()
        .filter(|argument| !argument.omittable())
        .map(|argument| format!("  {} is `{}`", argument.at, argument.ty))
        .collect();

    assert!(
        offenders.is_empty(),
        "these arguments name something the CLI can draw a roster of, and cannot be left \
         off:\n{}\n\
         A Selector omitted is a question — make it `Option<_>` and resolve it through the \
         noun's picker. A Filter omitted means every candidate, so it must be omittable to \
         mean anything, and belongs in DECLARATIONS with the listing it narrows. This is \
         what #922, #923, #924 and #925 each were (ADR-0079).",
        offenders.join("\n")
    );
}

#[test]
fn every_selector_reaches_its_own_picker() {
    let walked = modules();
    let calls = call_graph(&walked);
    let reached = handlers(&walked, &calls);

    let mut offenders: Vec<String> = Vec::new();
    for argument in candidates(&walked) {
        let Some(label) = selector_label(&argument) else {
            continue;
        };
        let Some(from) = reached.get(argument.variant()) else {
            offenders.push(format!("  {} — no match arm dispatches it", argument.at));
            continue;
        };
        if !calls.reaches_picker(from, &label) {
            offenders.push(format!(
                "  {} — nothing its handler reaches builds `Choice::new(\"{label}\")`",
                argument.at
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "these Selectors name the subject their command acts on, and an omitted one is \
         never asked for:\n{}\n\
         Resolve it through the picker for its noun; `hosts::select_or_arg` is the Host \
         one. Answering for the caller instead — a lone configured host, a bail — is \
         #911, where `backup verify -H` was already `Option<String>` and refused rather \
         than asked. If the argument narrows a listing instead, declare it a Filter in \
         DECLARATIONS with the reason (ADR-0079).",
        offenders.join("\n")
    );
}

#[test]
fn every_declaration_names_a_live_argument() {
    let walked = modules();
    let live: BTreeSet<String> = candidates(&walked)
        .into_iter()
        .map(|argument| argument.at)
        .collect();
    let stale: Vec<&str> = DECLARATIONS
        .iter()
        .map(|entry| entry.at)
        .filter(|at| !live.contains(*at))
        .collect();

    assert!(
        stale.is_empty(),
        "these DECLARATIONS excuse an argument the clap tree no longer has:\n  {}\n\
         A renamed argument keeps its old excuse and arrives unclassified, which is the \
         table outliving what it was written about.",
        stale.join("\n  ")
    );
}

/// The counterpart to [`every_declaration_names_a_live_argument`], and it was
/// missing until a review found two entries that fenced nothing.
///
/// `DECLARATIONS` was held against the tree from the start; `SUBJECT_NOUNS`
/// was not, and a vocabulary nothing checks is where `node` and `target` sat
/// — one the label a picker draws itself under rather than any argument's
/// name, the other reached only as a flatten and so recursed through. Both
/// read as coverage. Deleting either moved no other assertion, which is the
/// definition of decoration.
#[test]
fn every_subject_noun_names_a_live_argument() {
    let walked = modules();
    let declared: BTreeSet<String> = arguments(&walked)
        .into_iter()
        .map(|argument| argument.name)
        .collect();

    let dead: Vec<&str> = SUBJECT_NOUNS
        .iter()
        .copied()
        .filter(|noun| !declared.contains(*noun))
        .collect();

    assert!(
        dead.is_empty(),
        "these SUBJECT_NOUNS match no argument the clap tree declares:\n  {}\n\
         A noun no argument carries fences nothing and reads as coverage. Either an \
         argument was renamed — rename it here too — or the entry was never an argument \
         name: a picker's label (`node`, where the argument is `name`) or a flatten the \
         walk recurses through (`target`) belongs in DECLARATIONS or nowhere.",
        dead.join("\n  ")
    );
}

#[test]
fn every_declaration_gives_a_reason() {
    let thin: Vec<String> = DECLARATIONS
        .iter()
        .filter_map(|entry| match &entry.role {
            _ if entry.why.trim().is_empty() => Some(format!("{} — no reason given", entry.at)),
            Role::Implied(what) if what.trim().is_empty() => Some(format!(
                "{} — declared Implied without naming what answers for it",
                entry.at
            )),
            _ => None,
        })
        .collect();

    assert!(
        thin.is_empty(),
        "an excused argument must say what it is instead, and what answers when it is \
         omitted:\n  {}",
        thin.join("\n  ")
    );
}

/// The reach, as five counts. Without it the assertions above pass by finding
/// nothing whenever a parser stops seeing a shape — and the shapes here are a
/// `derive` macro's, not a token's, so a scan that matches nothing reports a
/// clean CLI rather than an empty one.
///
/// Five rather than one because they fail apart:
///
/// | Count       | Moves when                                              |
/// | ----------- | ------------------------------------------------------- |
/// | enums       | the derive scan breaks — and takes the other four with it |
/// | variants    | the variant scan breaks; the enums survive               |
/// | arguments   | the field scan or the flatten recursion breaks           |
/// | Subject Nouns | an argument is renamed, or the vocabulary changes      |
/// | Selectors   | an argument is reclassified in `DECLARATIONS`            |
///
/// The last is the one that moves on a judgement rather than a parser, which
/// is why it is counted separately from the Subject Nouns above it: turning a
/// Selector into a Filter is exactly the edit that should be hard to make
/// quietly. Stated as equality, not a floor, for `crate_source`'s reason — a
/// floor lets the domain shrink silently.
#[test]
fn the_walk_reaches_the_whole_command_surface() {
    let walked = modules();
    let enums = subcommand_enums(&walked);
    let variant_count: usize = enums.values().map(|body| variants(body).len()).sum();
    let all = arguments(&walked);
    let found = candidates(&walked);
    let selectors = found
        .iter()
        .filter(|argument| selector_label(argument).is_some())
        .count();

    assert_eq!(
        (
            enums.len(),
            variant_count,
            all.len(),
            found.len(),
            selectors
        ),
        (13, 64, 160, 66, 44),
        "the command surface moved: {} Subcommand enums, {} variants, {} arguments, {} of \
         them Subject Nouns, {} of those Selectors ({} excused or relabelled by name).\n\
         Enums: {:?}",
        enums.len(),
        variant_count,
        all.len(),
        found.len(),
        selectors,
        DECLARATIONS.len(),
        enums.keys().collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Witnesses: the scanners find what they claim to
// ---------------------------------------------------------------------------

/// Spelled as inputs and expectations rather than derived from the tree,
/// because a witness computed from the thing it witnesses asserts only that
/// the scanner agrees with itself.
#[test]
fn the_field_scanner_reads_attributes_names_and_generic_types() {
    let body = r#"
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        // a comment that is not a field
        #[arg(long, default_value = "admin", help = "FreshRSS username")]
        pub user: String,
        #[command(flatten)]
        target: TargetAddressArg,
        parameters: HashMap<String, bool>,
        apps: Option<Vec<String>>,
    "#;

    let read: Vec<(String, String)> = fields(body)
        .into_iter()
        .map(|field| (field.name, field.ty))
        .collect();

    assert_eq!(
        read,
        vec![
            ("host".to_string(), "Option<String>".to_string()),
            ("user".to_string(), "String".to_string()),
            ("target".to_string(), "TargetAddressArg".to_string()),
            // The comma inside the generics is the one a line-oriented split
            // reads as the end of the field.
            (
                "parameters".to_string(),
                "HashMap<String, bool>".to_string()
            ),
            ("apps".to_string(), "Option<Vec<String>>".to_string()),
        ]
    );
    assert!(fields(body)[1].attributes.contains("default_value"));
}

#[test]
fn a_bare_type_without_a_default_is_not_omittable() {
    let omittable = |attributes: &str, ty: &str| {
        Argument {
            at: "X::Y.z".to_string(),
            name: "z".to_string(),
            ty: ty.to_string(),
            attributes: attributes.to_string(),
        }
        .omittable()
    };

    // The shape all four of #922, #923, #924 and #925 were.
    assert!(!omittable("#[arg(long)]", "String"));
    assert!(omittable("#[arg(long)]", "Option<String>"));
    assert!(omittable("#[arg(long)]", "Vec<String>"));
    assert!(omittable(
        r#"#[arg(long, default_value = "admin")]"#,
        "String"
    ));
}

/// The rename trap, as the tree actually spells it. Without this the Host
/// picker is invisible to four commands that reach it.
#[test]
fn a_rename_inside_a_brace_group_is_followed() {
    let source = "use crate::hosts::{HOST_FLAG, HostManager, select_or_arg as hosts_select_or_arg};\n\
                  use crate::services::inventory::select_or_arg as inventory_select_or_arg;\n";

    let found: BTreeMap<String, String> = renames(source).into_iter().collect();

    assert_eq!(
        found.get("hosts_select_or_arg"),
        Some(&"select_or_arg".to_string())
    );
    assert_eq!(
        found.get("inventory_select_or_arg"),
        Some(&"select_or_arg".to_string())
    );

    let known = BTreeSet::from(["select_or_arg".to_string()]);
    assert_eq!(
        called(
            "let host = hosts_select_or_arg(arg, HOST_FLAG)?;",
            &known,
            &found
        ),
        vec!["select_or_arg".to_string()],
        "a call under the local name must resolve to the function it renames"
    );
}

/// The cut trap, on the file it bites. `src/hosts.rs` carries two
/// `#[cfg(test)]` attributes on production items long before its test module,
/// and `select_or_arg` sits between them and it.
#[test]
fn a_module_is_read_past_its_first_cfg_test() {
    let walked = modules();
    let hosts = crate_source::find(&walked, "src/hosts.rs");
    let kept = without_test_module(&hosts.source);

    assert!(
        hosts.source.find("#[cfg(test)]") < hosts.source.find("\n#[cfg(test)]\nmod tests"),
        "src/hosts.rs no longer has a #[cfg(test)] before its test module — this witness \
         has lost its subject and must be re-pointed at a file that does"
    );
    assert!(
        kept.contains("pub fn select_or_arg"),
        "cutting at the first #[cfg(test)] hides the Host picker, and with it every Host \
         Selector in the crate"
    );
    assert!(
        !kept.contains("mod tests"),
        "the trailing test module must still be cut: a test that calls a picker is not a \
         command reaching one"
    );

    // The anchor is one spelling, and a module that indents its test module or
    // names it something else keeps its test code in the call graph — where a
    // picker reached only from a test would read as a command reaching one.
    // That failure is silent, so it is asserted over the whole tree rather
    // than trusted from the one file above.
    let uncut: Vec<&str> = walked
        .iter()
        .filter(|module| without_test_module(&module.source).contains("mod tests"))
        .map(|module| module.repo_relative.as_str())
        .collect();
    assert!(
        uncut.is_empty(),
        "these modules keep a test module after the cut, so test code is in the call \
         graph:\n  {}\n\
         The anchor is `\\n#[cfg(test)]\\nmod tests`; a module spelling it otherwise needs \
         the anchor widened, not the assertion relaxed.",
        uncut.join("\n  ")
    );
}

#[test]
fn a_construction_is_not_read_as_a_dispatch() {
    let matched = "match cmd { BackupCommands::Verify { host, app } => run_backup_verify(host), }";
    let built = "let _verify = BackupCommands::Verify { host: None, app: None };";

    assert_eq!(
        arms(matched)
            .into_iter()
            .map(|(at, body)| (at, body.trim().to_string()))
            .collect::<Vec<_>>(),
        vec![(
            "BackupCommands::Verify".to_string(),
            "run_backup_verify(host)".to_string()
        )]
    );
    assert!(
        arms(built).is_empty(),
        "a variant built as a value dispatches nothing"
    );
}

/// Five Subject Nouns reach the clap tree only through a flatten or a tuple
/// variant, so a walk that reads the enum body alone misses them and passes.
#[test]
fn a_flatten_and_a_tuple_variant_both_yield_their_arguments() {
    let walked = modules();
    let reached: BTreeSet<String> = arguments(&walked)
        .into_iter()
        .map(|argument| argument.at)
        .collect();

    for at in [
        "DnsCommands::Migrate.target.host",
        "DnsCommands::Migrate.target.ip",
        "DnsCommands::SetAll.target.host",
        "Commands::Deploy.host",
        "Commands::Deploy.apps",
        "ConfigCommands::Init.playbooks",
    ] {
        assert!(reached.contains(at), "{at} must be walked");
    }
}

#[test]
fn a_handler_that_builds_no_picker_is_seen_as_reaching_none() {
    let walked = modules();
    let calls = call_graph(&walked);
    let reached = handlers(&walked, &calls);

    let verify = reached
        .get("BackupCommands::Verify")
        .expect("backup verify must dispatch");

    assert!(
        calls.reaches_picker(verify, "host"),
        "`backup verify -H` resolves through the Host picker since #911"
    );
    assert!(
        !calls.reaches_picker(verify, "synced folder"),
        "reaching one picker must not read as reaching every picker — otherwise a variant \
         with three Selectors is satisfied by any one of them (#923)"
    );
    assert!(
        !calls.reaches_picker(&BTreeSet::new(), "host"),
        "an empty reach reaches nothing"
    );
}
