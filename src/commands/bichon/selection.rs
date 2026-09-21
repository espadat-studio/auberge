use crate::prompt;
use eyre::Result;

/// Reads as a meta-entry rather than an address: every other picker row is an
/// email, and `[all apps]` already sets this spelling in `deploy`.
const ALL_ACCOUNTS: &str = "[all accounts]";

/// The `--account` filter, resolved. `None` selects every Account Bichon
/// reports.
///
/// This is the Account half of the pair every `bichon` subcommand resolves
/// its subject through; the Host half is `hosts::select_or_arg(host,
/// HOST_FLAG)`. Reach for both rather than answering either question again.
///
/// `--account` narrows a listing rather than naming the subject, so an
/// omitted one means "all" whenever no picker can be drawn — a script that
/// passed no filter yesterday still reaches every Account today. Where one
/// can, the same absence is a question worth asking, so the picker offers
/// `[all accounts]` above the roster.
///
/// A name Bichon does not report is an error, never an empty selection:
/// filtering a typo away leaves a run over nothing that reports success.
/// That is the shape #922 was filed for.
///
/// `can_prompt` is the caller's answer to "could a picker be drawn", and it
/// must come from [`prompt::is_interactive`] rather than a stdin-only check:
/// gating on stdin alone and then calling `select_item` makes `cmd 2>log`
/// refuse an omitted filter instead of reading it as "all".
pub fn resolve_account_filter(
    filter: Option<String>,
    known: &[String],
    can_prompt: bool,
) -> Result<Option<String>> {
    match filter {
        Some(email) => {
            if known.iter().any(|k| k == &email) {
                return Ok(Some(email));
            }
            let reported = if known.is_empty() {
                "no accounts".to_string()
            } else {
                known.join(", ")
            };
            eyre::bail!("unknown account '{email}'; Bichon reports {reported}")
        }
        // Nothing to ask about when Bichon reports nothing: `reconcile-folders`
        // runs against an empty roster and reports an empty plan.
        None if !can_prompt || known.is_empty() => Ok(None),
        None => {
            let mut items = Vec::with_capacity(known.len() + 1);
            items.push(ALL_ACCOUNTS.to_string());
            items.extend_from_slice(known);
            let choice = prompt::select_item(
                &items,
                |s: &String| s.clone(),
                prompt::Choice::new("account").resolved_by("--account <email>"),
            )?;
            Ok((choice != ALL_ACCOUNTS).then_some(choice))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Vec<String> {
        vec!["a@x.io".to_string(), "b@x.io".to_string()]
    }

    /// The shape #922 was filed for: an unknown address used to filter every
    /// Account away and let the run report success over nothing.
    #[test]
    fn explicit_account_must_be_one_bichon_reports() {
        let err =
            resolve_account_filter(Some("ghost@x.io".to_string()), &known(), false).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("ghost@x.io"), "{msg}");
        assert!(msg.contains("a@x.io"), "{msg}");
    }

    #[test]
    fn an_unknown_account_names_an_empty_roster_as_such() {
        let err = resolve_account_filter(Some("ghost@x.io".to_string()), &[], false).unwrap_err();
        assert!(format!("{err}").contains("no accounts"), "{err}");
    }

    #[test]
    fn explicit_known_account_narrows_to_it() {
        let filter = resolve_account_filter(Some("b@x.io".to_string()), &known(), false).unwrap();
        assert_eq!(filter.as_deref(), Some("b@x.io"));
    }

    #[test]
    fn omitted_account_off_a_tty_means_all_accounts() {
        assert_eq!(resolve_account_filter(None, &known(), false).unwrap(), None);
    }

    /// Mutation-test the `known.is_empty()` arm by deleting it: the picker is
    /// then drawn over a lone `[all accounts]` row.
    #[test]
    fn an_empty_roster_resolves_to_all_even_on_a_tty() {
        assert_eq!(resolve_account_filter(None, &[], true).unwrap(), None);
    }
}
