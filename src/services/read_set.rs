//! The variable walk over the Assets Tree: which names one run's files read,
//! and which roles it reaches. The runtime narrows `!` resolution to a run's
//! [`ReadSet`] through it, and `tests/variable_answerability.rs` checks every
//! unguarded name has an answer through the same walk, so the two cannot
//! disagree about what a run is made of.
//!
//! Every file is read structurally: only what sits inside `{{ … }}` and
//! `{% … %}`, plus the YAML keys whose scalar is itself an expression
//! (`when:` and its siblings). A file that does not read or parse is an
//! error, never skipped: a skipped file is a key the run reads and never gets.

use eyre::{Context, Result};
use regex::Regex;
use serde_yaml::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The config keys one run reads (`CONTEXT.md`). Only a `!` value is filtered
/// against it: a plain value reaches every run as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadSet {
    /// Some file the run reaches looks a variable up by a name it computes,
    /// so no key can be ruled out.
    Everything,
    Names(BTreeSet<String>),
}

impl ReadSet {
    pub fn contains(&self, key: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::Names(names) => names.contains(key),
        }
    }
}

/// Jinja's own words, plus the callables that appear as bare identifiers.
/// A name here is never a variable reference.
const JINJA_WORDS: &[&str] = &[
    "and",
    "as",
    "block",
    "break",
    "caller",
    "continue",
    "cycler",
    "dict",
    "dictsort",
    "do",
    "elif",
    "else",
    "endblock",
    "endfilter",
    "endfor",
    "endif",
    "endmacro",
    "endraw",
    "endset",
    "endwith",
    "extends",
    "False",
    "false",
    "filter",
    "for",
    "from",
    "if",
    "import",
    "in",
    "include",
    "is",
    "joiner",
    "kwargs",
    "lipsum",
    "list",
    "lookup",
    "loop",
    "macro",
    "namespace",
    "None",
    "none",
    "not",
    "or",
    "q",
    "query",
    "range",
    "raw",
    "recursive",
    "self",
    "set",
    "True",
    "true",
    "undefined",
    "varargs",
    "with",
];

/// YAML keys whose scalar *is* a Jinja expression, with no `{{ … }}` around it.
const BARE_EXPRESSION_KEYS: &[&str] = &[
    "assert",
    "changed_when",
    "failed_when",
    "that",
    "until",
    "when",
];

/// A role's subdirectories whose YAML ansible renders during a deploy.
/// `files/` is copied byte for byte, so `{{ … }}` in one is literal text.
const TEMPLATED_ROLE_DIRS: &[&str] = &["defaults", "handlers", "meta", "tasks", "vars"];

static LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"'[^']*'|"[^"]*""#).unwrap());
static IDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").unwrap());
static EXPRESSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\{\{(.*?)\}\}|\{%-?(.*?)-?%\}").unwrap());
static FOR_TARGETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\{%-?\s*for\s+(.+?)\s+in\s").unwrap());
static SET_TARGET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{%-?\s*set\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
/// `before` ends in `is` / `is not`, so what follows is a test, not a variable.
static TEST_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bis\s+(not\s+)?$").unwrap());
static DEFAULTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"([A-Za-z_][A-Za-z0-9_]*)((?:\s*\.\s*[A-Za-z0-9_]+|\s*\[[^\]]*\])*)\s*\|\s*(default|d)\b",
    )
    .unwrap()
});
static TESTED_DEFINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)((?:\s*\.\s*[A-Za-z0-9_]+)*)\s+is\s+(not\s+)?defined\b")
        .unwrap()
});
/// `lookup('vars', …)` and its spellings; the group is the plugin name.
static VARS_LOOKUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b(?:lookup|query|q)\s*\(\s*['"](?:ansible\.builtin\.)?(vars|varnames)['"]"#)
        .unwrap()
});
/// `vars[` or `hostvars[…][`, where `vars`/`hostvars` is a root name.
static VARS_SUBSCRIPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^\w.])(?:vars|hostvars\s*\[[^\]]*\])\s*\[").unwrap());
/// `vars.x` or `hostvars[…].x`: `x` is read, though it sits after a `.`.
static VARS_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^\w.])(?:vars|hostvars\s*\[[^\]]*\])\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)")
        .unwrap()
});
static QUOTED_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*(?:'([^']*)'|"([^"]*)")\s*$"#).unwrap());
static KEYWORD_ARG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[A-Za-z_][A-Za-z0-9_]*\s*=[^=]").unwrap());

/// String literals blanked out, so their contents are never read as names and
/// the offsets around them still line up.
fn mask_literals(expr: &str) -> String {
    let mut masked = String::with_capacity(expr.len());
    let mut cursor = 0;
    for found in LITERAL.find_iter(expr) {
        masked.push_str(&expr[cursor..found.start()]);
        masked.extend(found.as_str().chars().map(|_| ' '));
        cursor = found.end();
    }
    masked.push_str(&expr[cursor..]);
    masked
}

/// Every Jinja expression body in a blob of text.
fn expressions(text: &str) -> Vec<String> {
    EXPRESSION
        .captures_iter(text)
        .filter_map(|caps| caps.get(1).or_else(|| caps.get(2)))
        .map(|m| m.as_str().to_string())
        .collect()
}

/// The names a template binds for itself: `{% for x, y in … %}`, `{% set x = … %}`.
fn template_bindings(text: &str) -> BTreeSet<String> {
    let mut bound = BTreeSet::new();
    for caps in FOR_TARGETS.captures_iter(text) {
        for part in caps[1].split(',') {
            let part = part.trim().trim_matches(['(', ')']).trim();
            if IDENT.find(part).is_some_and(|m| m.as_str() == part) {
                bound.insert(part.to_string());
            }
        }
    }
    for caps in SET_TARGET.captures_iter(text) {
        bound.insert(caps[1].to_string());
    }
    bound
}

/// The variables an expression reads at their root.
///
/// An identifier is a root reference unless something in front of it says
/// otherwise: `.` makes it an attribute, `|` a filter, `is` a test, and a
/// following `=` a callable's keyword argument.
fn root_names(expr: &str) -> BTreeSet<String> {
    let masked = mask_literals(expr);
    let mut names = BTreeSet::new();
    for found in IDENT.find_iter(&masked) {
        let before = &masked[..found.start()];
        let trimmed = before.trim_end();
        if trimmed.ends_with('.') || trimmed.ends_with('|') || TEST_NAME.is_match(before) {
            continue;
        }
        let after = masked[found.end()..].trim_start();
        if after.starts_with('=') && !after.starts_with("==") {
            continue;
        }
        let name = found.as_str();
        if JINJA_WORDS.contains(&name) {
            continue;
        }
        names.insert(name.to_string());
    }
    names
}

/// The names this expression answers for itself: `x | default(…)`, and
/// `x is defined`, which is how a role reads an optional key.
fn guarded_names(expr: &str) -> BTreeSet<String> {
    let masked = mask_literals(expr);
    let mut guarded = BTreeSet::new();
    for caps in DEFAULTED.captures_iter(&masked) {
        guarded.insert(caps[1].to_string());
    }
    for caps in TESTED_DEFINED.captures_iter(&masked) {
        guarded.insert(caps[1].to_string());
    }
    guarded
}

const OPENERS: &str = "([{";
const CLOSERS: &str = ")]}";

/// The text from `open` (just past a `(` or `[`) to its matching closer,
/// split on top-level commas.
fn arguments(text: &str, open: usize) -> Vec<&str> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut start = open;
    let mut args = Vec::new();
    for (offset, ch) in text[open..].char_indices() {
        let at = open + offset;
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(ch),
            (None, c) if OPENERS.contains(c) => depth += 1,
            (None, c) if CLOSERS.contains(c) && depth == 0 => {
                args.push(&text[start..at]);
                return args;
            }
            (None, c) if CLOSERS.contains(c) => depth -= 1,
            (None, ',') if depth == 0 => {
                args.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    args.push(&text[start..]);
    args
}

fn quoted_name(arg: &str) -> Option<String> {
    let caps = QUOTED_NAME.captures(arg)?;
    Some(caps.get(1).or_else(|| caps.get(2))?.as_str().to_string())
}

/// What one file says: the names it reads, those it reads with no guard, the
/// names it binds, the roles it pulls in by name, and whether it reaches a
/// variable by a name it computes.
#[derive(Default)]
pub struct FileScan {
    pub names: BTreeSet<String>,
    pub unguarded: BTreeSet<String>,
    pub bound: BTreeSet<String>,
    pub included_roles: BTreeSet<String>,
    pub computed_access: bool,
}

impl FileScan {
    fn read_body(&mut self, expr: &str) {
        let guarded = guarded_names(expr);
        for name in root_names(expr) {
            if !guarded.contains(&name) {
                self.unguarded.insert(name.clone());
            }
            self.names.insert(name);
        }
        self.read_dynamic_access(expr);
    }

    /// A lookup by literal name reads that name; any other spelling of
    /// "the variable called <expression>" could read any key at all.
    fn read_dynamic_access(&mut self, expr: &str) {
        for caps in VARS_LOOKUP.captures_iter(expr) {
            if &caps[1] == "varnames" {
                self.computed_access = true;
                continue;
            }
            let after_plugin = caps.get(0).map_or(0, |m| m.end());
            for arg in arguments(expr, after_plugin).into_iter().skip(1) {
                if KEYWORD_ARG.is_match(arg) {
                    continue;
                }
                match quoted_name(arg) {
                    Some(name) => {
                        self.names.insert(name);
                    }
                    None => self.computed_access = true,
                }
            }
        }
        for found in VARS_SUBSCRIPT.find_iter(expr) {
            match arguments(expr, found.end())
                .first()
                .and_then(|a| quoted_name(a))
            {
                Some(name) => {
                    self.names.insert(name);
                }
                None => self.computed_access = true,
            }
        }
        for caps in VARS_ATTRIBUTE.captures_iter(expr) {
            self.names.insert(caps[1].to_string());
        }
    }

    /// Text that may hold `{{ … }}` / `{% … %}` among literal content.
    fn read_text(&mut self, text: &str) {
        self.bound.extend(template_bindings(text));
        for expr in expressions(text) {
            self.read_body(&expr);
        }
    }

    /// A scalar that is itself an expression — a `when:` and its siblings.
    fn read_expression(&mut self, expr: &str) {
        if EXPRESSION.is_match(expr) {
            self.read_text(expr);
            return;
        }
        self.read_body(expr);
    }

    fn walk(&mut self, node: &Value, bare: bool) {
        match node {
            Value::Mapping(map) => {
                for (key, value) in map {
                    let Some(key) = key.as_str() else { continue };
                    // `ansible.builtin.set_fact` and `set_fact` are one module.
                    match key.rsplit('.').next().unwrap_or(key) {
                        "set_fact" => self.bind_mapping_keys(value, &["cacheable"]),
                        "vars" => self.bind_mapping_keys(value, &[]),
                        "register" => {
                            if let Some(name) = value.as_str() {
                                self.bound.insert(name.to_string());
                            }
                        }
                        "loop_control" => {
                            if let Some(name) = value.get("loop_var").and_then(Value::as_str) {
                                self.bound.insert(name.to_string());
                            }
                        }
                        "include_role" | "import_role" => {
                            if value.is_string() {
                                self.computed_access = true;
                            }
                            if let Some(name) = value.get("name").and_then(Value::as_str) {
                                if EXPRESSION.is_match(name) {
                                    self.computed_access = true;
                                } else {
                                    self.included_roles.insert(name.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                    self.walk(value, BARE_EXPRESSION_KEYS.contains(&key));
                }
            }
            Value::Sequence(items) => {
                for item in items {
                    self.walk(item, bare);
                }
            }
            Value::String(text) => {
                if bare {
                    self.read_expression(text);
                } else {
                    self.read_text(text);
                }
            }
            _ => {}
        }
    }

    fn bind_mapping_keys(&mut self, value: &Value, skip: &[&str]) {
        let Value::Mapping(map) = value else { return };
        for key in map.keys() {
            if let Some(name) = key.as_str()
                && !skip.contains(&name)
            {
                self.bound.insert(name.to_string());
            }
        }
    }
}

fn is_yaml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "yml" || ext == "yaml")
}

/// One file's scan: YAML structurally, anything else as template text.
pub fn scan(path: &Path) -> Result<FileScan> {
    let raw =
        fs::read_to_string(path).wrap_err_with(|| format!("Failed to read {}", path.display()))?;
    let mut scan = FileScan::default();
    if is_yaml(path) {
        let parsed: Value = serde_yaml::from_str(&raw)
            .wrap_err_with(|| format!("Failed to parse {}", path.display()))?;
        scan.walk(&parsed, false);
    } else {
        scan.read_text(&raw);
    }
    Ok(scan)
}

fn collect(dir: &Path, keep: fn(&Path) -> bool, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries =
        fs::read_dir(dir).wrap_err_with(|| format!("Failed to read {}", dir.display()))?;
    for entry in entries {
        let path = entry
            .wrap_err_with(|| format!("Failed to read {}", dir.display()))?
            .path();
        if path.is_dir() {
            collect(&path, keep, out)?;
        } else if keep(&path) {
            out.push(path);
        }
    }
    Ok(())
}

/// Every `.yml`/`.yaml` under `dir`, at any depth, sorted. A missing
/// directory holds none.
pub fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect(dir, is_yaml, &mut files)?;
    files.sort();
    Ok(files)
}

/// Every file of a role a deploy renders: its templated YAML and its
/// `templates/`, whatever their extension.
pub fn role_files(ansible_dir: &Path, role: &str) -> Result<Vec<PathBuf>> {
    let dir = ansible_dir.join("roles").join(role);
    if !dir.is_dir() {
        eyre::bail!("role '{role}' is not in {}", dir.display());
    }
    let mut files = Vec::new();
    for sub in TEMPLATED_ROLE_DIRS {
        files.extend(yaml_files(&dir.join(sub))?);
    }
    collect(&dir.join("templates"), |p| p.is_file(), &mut files)?;
    files.sort();
    Ok(files)
}

/// The roles one role declares as dependencies in its `meta/main.yml`.
fn declared_dependencies(ansible_dir: &Path, role: &str) -> Result<BTreeSet<String>> {
    let path = ansible_dir.join("roles").join(role).join("meta/main.yml");
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let raw =
        fs::read_to_string(&path).wrap_err_with(|| format!("Failed to read {}", path.display()))?;
    let meta: Value = serde_yaml::from_str(&raw)
        .wrap_err_with(|| format!("Failed to parse {}", path.display()))?;
    Ok(meta
        .get("dependencies")
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(|entry| match entry {
                    Value::String(name) => Some(name.clone()),
                    other => other
                        .get("role")
                        .or_else(|| other.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Every role a run over `playbook` reaches from `seed`: the seed plus the
/// playbook's own `include_role`s, grown through `meta/main.yml`
/// dependencies and `include_role` until it stops growing.
pub fn run_roles(
    ansible_dir: &Path,
    playbook: &Path,
    seed: impl IntoIterator<Item = String>,
) -> Result<BTreeSet<String>> {
    let mut roles: BTreeSet<String> = seed.into_iter().collect();
    roles.extend(scan(playbook)?.included_roles);
    let mut pending: Vec<String> = roles.iter().cloned().collect();
    while let Some(role) = pending.pop() {
        let mut edges = declared_dependencies(ansible_dir, &role)?;
        for path in role_files(ansible_dir, &role)? {
            edges.extend(scan(&path)?.included_roles);
        }
        for edge in edges {
            if roles.insert(edge.clone()) {
                pending.push(edge);
            }
        }
    }
    Ok(roles)
}

/// The [`ReadSet`] of one run: every name, guarded or not, read by the
/// playbook, by every role [`run_roles`] reaches from `seed`, by
/// `group_vars/` and by `inventory.yml`, which every run loads. Any computed
/// lookup among them makes it [`ReadSet::Everything`].
pub fn read_set(
    ansible_dir: &Path,
    playbook: &Path,
    seed: impl IntoIterator<Item = String>,
) -> Result<ReadSet> {
    let mut files = vec![playbook.to_path_buf()];
    for role in run_roles(ansible_dir, playbook, seed)? {
        files.extend(role_files(ansible_dir, &role)?);
    }
    let group_vars = ansible_dir.join("group_vars");
    let inventory = ansible_dir.join("inventory.yml");
    for every_run in [&group_vars, &inventory] {
        if !every_run.exists() {
            eyre::bail!("{} is missing; every run reads it", every_run.display());
        }
    }
    files.extend(yaml_files(&group_vars)?);
    files.push(inventory);
    let mut names = BTreeSet::new();
    for path in files {
        let scan = scan(&path)?;
        if scan.computed_access {
            return Ok(ReadSet::Everything);
        }
        names.extend(scan.names);
    }
    Ok(ReadSet::Names(names))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// `app` reads `app_key` and depends on `dep`; `dep` includes `inc`;
    /// `other` sits on the roster but is not seeded.
    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "playbooks/site.yml",
            "- hosts: all\n  vars:\n    who: \"{{ play_key }}\"\n  roles:\n    - role: app\n    - role: other\n",
        );
        write(
            root,
            "group_vars/all/main.yml",
            "fleet: \"{{ group_key }}\"\n",
        );
        write(
            root,
            "inventory.yml",
            "all:\n  vars:\n    x: \"{{ inventory_key }}\"\n",
        );
        write(
            root,
            "roles/app/meta/main.yml",
            "dependencies:\n  - role: dep\n",
        );
        write(
            root,
            "roles/app/tasks/main.yml",
            "- name: t\n  ansible.builtin.debug:\n    msg: \"{{ app_key }}\"\n  when: guarded_key is defined\n",
        );
        write(
            root,
            "roles/app/templates/app.conf.j2",
            "x={{ template_key | default('') }}\n",
        );
        write(
            root,
            "roles/dep/tasks/main.yml",
            "- ansible.builtin.include_role:\n    name: inc\n",
        );
        write(
            root,
            "roles/inc/defaults/main.yml",
            "inc_default: \"{{ inc_key }}\"\n",
        );
        write(
            root,
            "roles/other/tasks/main.yml",
            "- ansible.builtin.debug:\n    msg: \"{{ other_key }}\"\n",
        );
        dir
    }

    fn read(root: &Path, seed: &[&str]) -> ReadSet {
        read_set(
            root,
            &root.join("playbooks/site.yml"),
            seed.iter().map(|s| s.to_string()),
        )
        .unwrap()
    }

    #[test]
    fn test_read_set_follows_dependencies_includes_and_shared_files() {
        let dir = tree();
        let set = read(dir.path(), &["app"]);
        for key in [
            "app_key",
            "guarded_key",
            "template_key",
            "inc_key",
            "play_key",
            "group_key",
            "inventory_key",
        ] {
            assert!(set.contains(key), "{key} missing from {set:?}");
        }
    }

    #[test]
    fn test_read_set_leaves_out_roles_the_seed_does_not_reach() {
        let dir = tree();
        assert!(!read(dir.path(), &["app"]).contains("other_key"));
        assert!(read(dir.path(), &["app", "other"]).contains("other_key"));
    }

    #[test]
    fn test_literal_lookup_reads_its_name() {
        let dir = tree();
        write(
            dir.path(),
            "roles/app/tasks/extra.yml",
            "- ansible.builtin.debug:\n    msg: \"{{ lookup('vars', 'looked_up_key') }}\"\n",
        );
        let set = read(dir.path(), &["app"]);
        assert!(set.contains("looked_up_key"), "{set:?}");
        assert_ne!(set, ReadSet::Everything);
    }

    #[test]
    fn test_every_computed_access_form_reads_everything() {
        for form in [
            "lookup('vars', name_var)",
            "lookup('ansible.builtin.vars', name_var, default='')",
            "query('vars', 'a', name_var)",
            "q('vars', name_var)",
            "lookup('varnames', '^x_')",
            "vars[name_var]",
            "hostvars[inventory_hostname][name_var]",
            "lookup('vars', 'a') ~ lookup('vars', name_var)",
        ] {
            let dir = tree();
            write(
                dir.path(),
                "roles/inc/tasks/main.yml",
                &format!("- ansible.builtin.debug:\n    msg: \"{{{{ {form} }}}}\"\n"),
            );
            assert_eq!(read(dir.path(), &["app"]), ReadSet::Everything, "{form}");
        }
    }

    #[test]
    fn test_attribute_on_vars_or_hostvars_reads_the_attribute() {
        let dir = tree();
        write(
            dir.path(),
            "roles/app/tasks/extra.yml",
            "- ansible.builtin.debug:\n    msg: \"{{ vars.attr_key }} {{ hostvars[h].host_attr_key }}\"\n",
        );
        let set = read(dir.path(), &["app"]);
        assert!(
            set.contains("attr_key") && set.contains("host_attr_key"),
            "{set:?}"
        );
    }

    #[test]
    fn test_free_form_include_role_reads_everything() {
        let dir = tree();
        write(
            dir.path(),
            "roles/inc/tasks/main.yml",
            "- ansible.builtin.include_role: name=dep\n",
        );
        assert_eq!(read(dir.path(), &["app"]), ReadSet::Everything);
    }

    #[test]
    fn test_missing_inventory_is_an_error() {
        let dir = tree();
        fs::remove_file(dir.path().join("inventory.yml")).unwrap();
        let err = read_set(
            dir.path(),
            &dir.path().join("playbooks/site.yml"),
            ["app".to_string()],
        )
        .unwrap_err();
        assert!(err.to_string().contains("inventory.yml"), "{err}");
    }

    #[test]
    fn test_computed_role_name_reads_everything() {
        let dir = tree();
        write(
            dir.path(),
            "roles/inc/tasks/main.yml",
            "- ansible.builtin.include_role:\n    name: \"{{ which }}\"\n",
        );
        assert_eq!(read(dir.path(), &["app"]), ReadSet::Everything);
    }

    #[test]
    fn test_unparsable_role_file_is_an_error() {
        let dir = tree();
        write(dir.path(), "roles/inc/tasks/main.yml", "- [unclosed\n");
        let err = read_set(
            dir.path(),
            &dir.path().join("playbooks/site.yml"),
            ["app".to_string()],
        )
        .unwrap_err();
        assert!(
            format!("{err:?}").contains("roles/inc/tasks/main.yml"),
            "{err:?}"
        );
    }

    #[test]
    fn test_unknown_role_is_an_error() {
        let dir = tree();
        let err = read_set(
            dir.path(),
            &dir.path().join("playbooks/site.yml"),
            ["ghost".to_string()],
        )
        .unwrap_err();
        assert!(err.to_string().contains("ghost"), "{err}");
    }
}
