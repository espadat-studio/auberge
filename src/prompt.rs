use crate::output::should_use_colors;
use dialoguer::{
    Confirm, FuzzySelect, Input, MultiSelect, Select, theme::ColorfulTheme, theme::SimpleTheme,
};
use eyre::Result;
use std::io::IsTerminal;

#[derive(Debug, PartialEq, Eq)]
enum ThemeKind {
    Colorful,
    Simple,
}

fn theme_kind() -> ThemeKind {
    if should_use_colors() {
        ThemeKind::Colorful
    } else {
        ThemeKind::Simple
    }
}

fn dialoguer_theme() -> Box<dyn dialoguer::theme::Theme> {
    match theme_kind() {
        ThemeKind::Colorful => Box::new(ColorfulTheme::default()),
        ThemeKind::Simple => Box::new(SimpleTheme),
    }
}

/// Whether a picker can be drawn at all. Public because a caller that
/// decides *whether to ask* must gate on the same predicate `select_item`
/// does: `HostManager::is_tty` reads stdin alone, so gating on it and then
/// calling `select_item` turns `cmd 2>log` into a refusal where the caller
/// meant to skip the question.
pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Describes what is being chosen so `select_item` can render each of the
/// three ways a pick can fail to happen as its own actionable error.
///
/// Returning one `Option` for all three let call sites collapse "nothing to
/// choose from", "cannot draw a picker" and "user dismissed the picker" into
/// one misleading message.
///
/// `noun` is pluralised as `{noun}s`; every candidate kind in this tree
/// pluralises regularly.
pub struct Choice {
    prompt: String,
    noun: String,
    argument: Option<String>,
    populate: Option<String>,
}

impl Choice {
    pub fn new(noun: &str) -> Self {
        Self {
            prompt: format!("Select {}", noun),
            noun: noun.to_string(),
            argument: None,
            populate: None,
        }
    }

    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.prompt = prompt.to_string();
        self
    }

    /// How the caller supplies the value when no picker can be drawn, e.g.
    /// `-H <host>`.
    pub fn resolved_by(mut self, argument: &str) -> Self {
        self.argument = Some(argument.to_string());
        self
    }

    /// Command that creates candidates when there are none, e.g.
    /// `auberge host add`.
    pub fn populated_by(mut self, command: &str) -> Self {
        self.populate = Some(command.to_string());
        self
    }

    fn no_candidates(&self) -> eyre::Report {
        match &self.populate {
            Some(command) => {
                eyre::eyre!("No {}s configured — run `{}`", self.noun, command)
            }
            None => eyre::eyre!("No {}s configured", self.noun),
        }
    }

    /// `candidates` is the picker's own rendered rows, so the operator reads
    /// the values a picker would have offered instead of running a second
    /// command to learn them.
    fn not_interactive(&self, candidates: &[String]) -> eyre::Report {
        let hint = match &self.argument {
            Some(argument) => format!(" — pass {}", argument),
            None => String::new(),
        };

        eyre::eyre!(
            "{} {}s to choose from and stdin is not a terminal{}: {}",
            candidates.len(),
            self.noun,
            hint,
            name_candidates(candidates)
        )
    }

    fn aborted(&self) -> eyre::Report {
        eyre::eyre!("No {} selected", self.noun)
    }
}

/// `config get` offers one candidate per Key Registry entry — 70 of them
/// today — and the reader on the non-interactive path is a log rather than
/// someone who can scroll.
///
/// Not every picker can reach this error: `headscale`, `backup`'s ID picker
/// and `bichon`'s account filter answer for themselves when no picker can be
/// drawn and never call `select_item`, so the cap is sized for the callers
/// that do reach it. `bichon`'s host picker stopped being one of them in
/// #922.
const MAX_NAMED_CANDIDATES: usize = 10;

fn name_candidates(candidates: &[String]) -> String {
    if candidates.len() <= MAX_NAMED_CANDIDATES {
        return candidates.join(", ");
    }

    format!(
        "{}, and {} more",
        candidates[..MAX_NAMED_CANDIDATES].join(", "),
        candidates.len() - MAX_NAMED_CANDIDATES
    )
}

/// Picks one of `items`, or fails with the reason no pick happened.
///
/// A lone candidate is auto-selected without a TTY: the choice is not a choice.
///
/// `display_fn` runs for every item even when no picker is drawn: the
/// non-interactive error names the same rows the picker would have listed,
/// and rendering them a second time would let the two drift.
pub fn select_item<T, F>(items: &[T], display_fn: F, choice: Choice) -> Result<T>
where
    T: Clone,
    F: Fn(&T) -> String,
{
    if items.is_empty() {
        return Err(choice.no_candidates());
    }

    let display_items: Vec<String> = items.iter().map(&display_fn).collect();

    if !is_interactive() {
        if let [only] = items {
            return Ok(only.clone());
        }
        return Err(choice.not_interactive(&display_items));
    }

    let theme = dialoguer_theme();

    // `.default(0)` is load-bearing: `FuzzySelect` only accepts Enter while a
    // row is highlighted, and it starts with none highlighted, so without it
    // the first keypress has to be an arrow before Enter does anything.
    let picked = FuzzySelect::with_theme(theme.as_ref())
        .with_prompt(&choice.prompt)
        .items(&display_items)
        .default(0)
        .interact_opt()
        .ok()
        .flatten()
        .ok_or_else(|| choice.aborted())?;

    Ok(items[picked].clone())
}

/// Picks any number of `items`, or `None` if the picker was dismissed with
/// nothing checked.
///
/// A lone candidate is auto-selected without a TTY, mirroring [`select_item`].
/// Toggling is space, not tab: tab moves the cursor down.
pub fn select_multi(items: &[String], prompt: &str) -> Option<Vec<String>> {
    if items.is_empty() {
        return None;
    }

    if !is_interactive() {
        if let [only] = items {
            return Some(vec![only.clone()]);
        }
        return None;
    }

    let theme = dialoguer_theme();
    let picked = MultiSelect::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .items(items)
        .interact_opt()
        .ok()
        .flatten()?;

    if picked.is_empty() {
        return None;
    }

    Some(picked.iter().map(|&i| items[i].clone()).collect())
}

/// A required free-text answer.
pub fn text(prompt: &str) -> Result<String> {
    let theme = dialoguer_theme();
    Ok(Input::<String>::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .interact_text()?)
}

/// A required free-text answer, or `arg` when the caller already has one.
///
/// The free-text counterpart to [`crate::hosts::select_or_arg`], for a value
/// with no candidate list to pick from (#924). It carries the same no-TTY
/// discipline [`select_item`] does, and for a sharper reason: dialoguer reads
/// the answer keystroke by keystroke off a `Term`, so without a terminal the
/// prompt does not fail, it spins. `argument` is how the caller supplies the
/// value instead, mirroring [`Choice::resolved_by`] — `Choice` itself does
/// not fit, since its `noun` and `populate` describe candidates there are
/// none of here.
pub fn text_or_arg(arg: Option<String>, prompt: &str, argument: &str) -> Result<String> {
    match arg {
        Some(value) => Ok(value),
        None => {
            eyre::ensure!(
                is_interactive(),
                "No answer for '{}' and stdin is not a terminal — pass {}",
                prompt,
                argument
            );
            text(prompt)
        }
    }
}

/// A free-text answer with `default` offered, which an empty line accepts.
///
/// Distinct from [`text_or_empty`] because dialoguer treats an empty line as
/// "take the default": a field that must end up non-empty and a field that may
/// be blanked are different prompts, not one prompt with a flag.
pub fn text_with_default(prompt: &str, default: String) -> Result<String> {
    let theme = dialoguer_theme();
    Ok(Input::<String>::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .default(default)
        .interact_text()?)
}

/// A free-text answer with `default` offered, where empty is a valid answer.
pub fn text_or_empty(prompt: &str, default: String) -> Result<String> {
    let theme = dialoguer_theme();
    Ok(Input::<String>::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .default(default)
        .allow_empty(true)
        .interact_text()?)
}

/// A free-text answer pre-filled with `initial`, which the operator can clear.
///
/// `.default()` cannot express this: dialoguer re-reads the default when the
/// line is empty, so a defaulted field can never be blanked. Pre-filling the
/// edit buffer instead leaves backspace as the way to clear it, which is what
/// an optional Host field needs.
pub fn text_prefilled(prompt: &str, initial: String) -> Result<String> {
    let theme = dialoguer_theme();
    Ok(Input::<String>::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .with_initial_text(initial)
        .allow_empty(true)
        .interact_text()?)
}

/// A port answer with `default` offered.
pub fn number_with_default(prompt: &str, default: u16) -> Result<u16> {
    let theme = dialoguer_theme();
    Ok(Input::<u16>::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .default(default)
        .interact_text()?)
}

/// Picks one of a closed, short set by arrow key, `default` preselected.
///
/// Deliberately not [`select_item`]'s fuzzy picker: both callers offer four or
/// five fixed entries where the answer is usually "keep what is already set",
/// and a filter box over five rows costs a keystroke to say nothing.
///
/// Returns the index, because a caller that maps entries onto a domain type
/// owns that mapping — see `host`'s `tier_item_index`/`tier_at_item` round trip.
pub fn pick_index<T: ToString>(prompt: &str, items: &[T], default: usize) -> Result<usize> {
    let theme = dialoguer_theme();
    Ok(Select::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .items(items)
        .default(default)
        .interact()?)
}

/// A yes/no answer with a caller-chosen default.
///
/// [`confirm`] hardcodes `false` and refuses without a TTY, which is right for
/// a guard on a destructive action. This one carries the current value as the
/// default, for an edit prompt that is offering a setting rather than gating.
pub fn confirm_default(prompt: &str, default: bool) -> Result<bool> {
    let theme = dialoguer_theme();
    Ok(Confirm::with_theme(theme.as_ref())
        .with_prompt(prompt)
        .default(default)
        .interact()?)
}

pub fn confirm(msg: &str, yes_flag: bool) -> bool {
    if yes_flag {
        return true;
    }

    if !std::io::stdin().is_terminal() {
        return false;
    }

    let theme = dialoguer_theme();
    Confirm::with_theme(theme.as_ref())
        .with_prompt(msg)
        .default(false)
        .interact()
        .unwrap_or(false)
}

/// Severe confirmation: the user must type `expected` exactly to proceed.
/// Use for irreversible / production-impacting actions.
///
/// Honors `yes_flag` (skip prompt, proceed) and non-TTY stdin (refuse, return
/// `Ok(false)` so callers can bail with an actionable message instead of
/// hanging on a prompt that nobody can answer).
pub fn confirm_typed(prompt_msg: &str, expected: &str, yes_flag: bool) -> Result<bool> {
    if yes_flag {
        return Ok(true);
    }

    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }

    let theme = dialoguer_theme();
    let typed: String = Input::with_theme(theme.as_ref())
        .with_prompt(prompt_msg)
        .allow_empty(true)
        .interact_text()?;

    Ok(typed.trim() == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{TEST_LOCK, set_no_color};

    fn hosts() -> Vec<String> {
        vec!["auberge".to_string(), "hermes".to_string()]
    }

    #[test]
    fn select_item_reports_nothing_configured_for_an_empty_list() {
        // The bug behind #468: an empty candidate list used to yield the same
        // "No host selected" as a dismissed picker, so `sync music`'s bogus
        // group filter looked like the user had declined to choose.
        let empty: Vec<String> = vec![];
        let err = select_item(
            &empty,
            |s: &String| s.clone(),
            Choice::new("host").populated_by("auberge host add"),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "No hosts configured — run `auberge host add`"
        );
    }

    #[test]
    fn select_item_omits_the_populate_hint_when_none_is_given() {
        let empty: Vec<String> = vec![];
        let err = select_item(&empty, |s: &String| s.clone(), Choice::new("playbook")).unwrap_err();

        assert_eq!(err.to_string(), "No playbooks configured");
    }

    #[test]
    fn select_item_names_the_argument_when_it_cannot_prompt() {
        // `cargo test` runs without a TTY, so this is the scripted path: two
        // candidates, no picker possible, and the error must name the flag
        // that resolves it.
        let err = select_item(
            &hosts(),
            |s: &String| s.clone(),
            Choice::new("host").resolved_by("-H <host>"),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "2 hosts to choose from and stdin is not a terminal — pass -H <host>: auberge, hermes"
        );
    }

    #[test]
    fn select_item_still_reports_the_count_without_an_argument_hint() {
        let err = select_item(&hosts(), |s: &String| s.clone(), Choice::new("host")).unwrap_err();

        assert_eq!(
            err.to_string(),
            "2 hosts to choose from and stdin is not a terminal: auberge, hermes"
        );
    }

    #[test]
    fn select_item_names_candidates_the_way_the_picker_draws_them() {
        // The names come from display_fn, the same rows FuzzySelect would
        // have listed. Formatting them a second time here would let the
        // error and the picker drift apart.
        let hosts = vec![
            ("auberge".to_string(), "203.0.113.10".to_string()),
            ("hermes".to_string(), "198.51.100.7".to_string()),
        ];
        let err = select_item(
            &hosts,
            |(name, ip): &(String, String)| format!("{} ({})", name, ip),
            Choice::new("host").resolved_by("-H <host>"),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "2 hosts to choose from and stdin is not a terminal — pass -H <host>: auberge (203.0.113.10), hermes (198.51.100.7)"
        );
    }

    #[test]
    fn select_item_names_every_candidate_up_to_the_cap() {
        let keys: Vec<String> = (1..=10).map(|n| format!("key-{}", n)).collect();
        let err =
            select_item(&keys, |s: &String| s.clone(), Choice::new("config key")).unwrap_err();

        assert_eq!(
            err.to_string(),
            "10 config keys to choose from and stdin is not a terminal: key-1, key-2, key-3, key-4, key-5, key-6, key-7, key-8, key-9, key-10"
        );
    }

    #[test]
    fn select_item_caps_a_long_candidate_list_and_counts_the_withheld() {
        let keys: Vec<String> = (1..=13).map(|n| format!("key-{}", n)).collect();
        let err = select_item(
            &keys,
            |s: &String| s.clone(),
            Choice::new("config key").resolved_by("the key as an argument"),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "13 config keys to choose from and stdin is not a terminal — pass the key as an argument: key-1, key-2, key-3, key-4, key-5, key-6, key-7, key-8, key-9, key-10, and 3 more"
        );
    }

    #[test]
    fn select_item_auto_selects_a_lone_candidate_without_a_tty() {
        // Deliberate: one candidate is not a choice, so scripts need no flag.
        // With the two tests above, this is the whole no-TTY policy that
        // `backup verify` leans on since #911 and `bichon`'s host argument
        // since #922 — asserted here, not per command.
        let only = vec!["auberge".to_string()];
        let selected = select_item(
            &only,
            |s: &String| s.clone(),
            Choice::new("host").resolved_by("-H <host>"),
        )
        .unwrap();

        assert_eq!(selected, "auberge");
    }

    #[test]
    fn text_or_arg_passes_a_given_answer_straight_through() {
        assert_eq!(
            text_or_arg(Some("vieille-auberge".to_string()), "New host name", "x").unwrap(),
            "vieille-auberge"
        );
    }

    /// The third leg of the no-TTY policy, alongside `select_item`'s two: a
    /// free-text answer nobody can type is refused, naming how to supply it.
    /// Deleting the `ensure!` does not fail this test, it hangs it —
    /// dialoguer spins on a `Term` it cannot read a key from, which is the
    /// failure the guard exists to prevent.
    #[test]
    fn text_or_arg_refuses_rather_than_prompts_without_a_tty() {
        let err = text_or_arg(None, "New host name", "the name as an argument")
            .unwrap_err()
            .to_string();

        assert_eq!(
            err,
            "No answer for 'New host name' and stdin is not a terminal — pass the name as an argument"
        );
    }

    #[test]
    fn select_multi_auto_selects_a_lone_candidate_without_a_tty() {
        // Same rule as select_item: one candidate is not a choice, so
        // `backup restore` needs no -a on a single-app backup.
        let only = vec!["paperless".to_string()];

        assert_eq!(select_multi(&only, "Select apps"), Some(only.clone()));
    }

    #[test]
    fn select_multi_declines_to_guess_between_candidates_without_a_tty() {
        // No picker can be drawn and no candidate is privileged, so callers
        // get None and surface their own actionable error.
        assert_eq!(select_multi(&hosts(), "Select apps"), None);
    }

    #[test]
    fn select_multi_reports_nothing_to_pick_for_an_empty_list() {
        assert_eq!(select_multi(&[], "Select apps"), None);
    }

    #[test]
    fn choice_defaults_its_prompt_from_the_noun() {
        assert_eq!(Choice::new("subdomain").prompt, "Select subdomain");
        assert_eq!(
            Choice::new("subdomain").with_prompt("Pick one").prompt,
            "Pick one"
        );
    }

    #[test]
    fn confirm_short_circuits_to_true_when_yes_flag_set() {
        assert!(confirm("anything", true));
    }

    #[test]
    fn confirm_returns_false_in_non_tty_without_yes_flag() {
        // `cargo test` runs with non-TTY stdin, so the is_terminal() guard
        // takes effect.  This is the path that prevents `dns set-all` and
        // `dns delete` from hanging in CI when --yes is omitted.
        assert!(!confirm("anything", false));
    }

    #[test]
    fn confirm_typed_short_circuits_to_true_when_yes_flag_set() {
        // --yes must bypass the typed-confirmation gate so CI can run without
        // a TTY attached.  Expected value is irrelevant on this path.
        assert!(confirm_typed("type the name", "freshrss", true).unwrap());
    }

    #[test]
    fn confirm_typed_returns_false_in_non_tty_without_yes_flag() {
        // Without --yes and without a TTY, severe confirmation cannot be
        // satisfied — callers should treat this as cancellation and surface
        // an actionable error rather than dispatching the destructive op.
        assert!(!confirm_typed("type the name", "freshrss", false).unwrap());
    }

    #[test]
    fn theme_kind_is_simple_when_no_color_flag_set() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_no_color(true);
        assert_eq!(theme_kind(), ThemeKind::Simple);
        set_no_color(false);
    }

    #[test]
    fn dialoguer_theme_does_not_panic_in_either_branch() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_no_color(true);
        let _simple = dialoguer_theme();
        set_no_color(false);
        let _maybe_colorful = dialoguer_theme();
    }
}
