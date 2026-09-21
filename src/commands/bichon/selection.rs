use crate::prompt;
use crate::services::bichon::api::Account;
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
        Some(email) if known.iter().any(|k| k == &email) => Ok(Some(email)),
        Some(email) => Err(unknown_account(&email, known)),
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

/// The one refusal both Account resolvers hand back, so a typo reads the same
/// whether it narrowed a listing or named a subject.
///
/// Sorted here so the list it names is the list a picker would have drawn,
/// whatever order the roster arrived in.
fn unknown_account(email: &str, known: &[String]) -> eyre::Report {
    let mut reported: Vec<&str> = known.iter().map(String::as_str).collect();
    reported.sort_unstable();
    let reported = if reported.is_empty() {
        "no accounts".to_string()
    } else {
        reported.join(", ")
    };
    eyre::eyre!("unknown account '{email}'; Bichon reports {reported}")
}

/// The Account as the *subject* of a command rather than a filter over a
/// listing: `verify-coverage` proves one Account's coverage, so
/// `[all accounts]` is not an answer it can take and an omitted `--account`
/// is a question rather than a default.
///
/// The subject counterpart to [`resolve_account_filter`], and the Account half
/// of the pair a `bichon` subcommand resolves through; the Host half is
/// `hosts::select_or_arg(host, HOST_FLAG)` either way. Both halves of the
/// Account question refuse an address Bichon does not report through
/// [`unknown_account`]: filtering — or verifying — a typo away leaves a run
/// over nothing that reports success (#922).
///
/// The no-TTY policy is [`prompt::select_item`]'s, unchanged: a lone Account
/// is implied and several error naming `--account`.
///
/// The roster is ordered here rather than trusted from the API or from the
/// caller, so the picker's rows and the refusal's list read the same way
/// whoever asks.
pub fn select_account(arg: Option<String>, accounts: &[Account]) -> Result<Account> {
    let mut roster = accounts.to_vec();
    roster.sort_by(|a, b| a.email.cmp(&b.email));

    match arg {
        Some(email) => roster
            .into_iter()
            .find(|a| a.email == email)
            .ok_or_else(|| {
                let known: Vec<String> = accounts.iter().map(|a| a.email.clone()).collect();
                unknown_account(&email, &known)
            }),
        None => {
            // `select_item` would call this "No accounts configured", which
            // reads as auberge's own config; the roster is Bichon's.
            eyre::ensure!(!roster.is_empty(), "Bichon reports no accounts");
            prompt::select_item(
                &roster,
                |a: &Account| a.email.clone(),
                prompt::Choice::new("account").resolved_by("--account <email>"),
            )
        }
    }
}

/// The Folder whose coverage to prove, from the Account's **Synced Folders**
/// and nothing else.
///
/// The Email Archive ingests a Synced Folder continuously, so it is the only
/// folder whose coverage it can vouch for: sidecars for a folder that has
/// left the set linger in the append-only Archive and would answer on stale
/// evidence. An explicit `--folder` is checked against the same set the
/// picker draws, because letting one through instead matches no store
/// message and reports `covered` over nothing — #922's shape with a folder
/// in place of an address.
///
/// The set is the `sync_folders` Bichon holds: what Account Reconcile last
/// wrote, and what the Archive is ingesting now. The Expunge Sweep starts
/// from the same set and then narrows it — re-reconciling against the live
/// folder list and refusing a folder pending removal — because it is about
/// to delete mail. This command only reports, so it stops at the set.
///
/// The set is sorted here rather than trusted from the API, so the picker's
/// rows and the refusal's list read in one order.
pub fn select_synced_folder(arg: Option<String>, account: &Account) -> Result<String> {
    let mut synced = account.sync_folders.clone();
    synced.sort();

    // Both arms need this, so it is asked once: an Account that syncs nothing
    // has no coverage to prove, whether or not a folder was named.
    eyre::ensure!(
        !synced.is_empty(),
        "{} syncs no folder, so the Email Archive can vouch for nothing — run `auberge bichon reconcile-folders --apply`",
        account.email
    );

    match arg {
        Some(folder) if synced.contains(&folder) => Ok(folder),
        Some(folder) => eyre::bail!(
            "'{folder}' is not a Synced Folder of {}; it syncs {}",
            account.email,
            synced.join(", ")
        ),
        None => prompt::select_item(
            &synced,
            |f: &String| f.clone(),
            prompt::Choice::new("synced folder").resolved_by("--folder <name>"),
        ),
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

    /// Deliberately out of order, so a dropped sort shows up in the rows the
    /// refusals name.
    fn accounts() -> Vec<Account> {
        vec![
            Account {
                id: 2,
                email: "b@x.io".to_string(),
                sync_folders: Vec::new(),
            },
            Account {
                id: 1,
                email: "a@x.io".to_string(),
                sync_folders: vec!["Sent".to_string(), "INBOX".to_string()],
            },
        ]
    }

    /// The Account with two Synced Folders; `accounts()` holds it second so
    /// the ordering assertions below have something to prove.
    fn synced() -> Account {
        accounts().remove(1)
    }

    #[test]
    fn an_explicit_account_resolves_to_its_record() {
        let account = select_account(Some("a@x.io".to_string()), &accounts()).unwrap();
        assert_eq!(account.id, 1);
        assert_eq!(account.sync_folders, ["Sent", "INBOX"]);
    }

    /// The subject path rejects a typo through the same error the filter path
    /// does, so #922's false success cannot come back on this side.
    #[test]
    fn an_unknown_subject_account_reports_what_bichon_does() {
        let err = select_account(Some("ghost@x.io".to_string()), &accounts()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("ghost@x.io"), "{msg}");
        assert!(msg.contains("a@x.io, b@x.io"), "{msg}");
    }

    /// `cargo test` runs without a TTY, so this and the next assert the
    /// scripted path. The policy is `select_item`'s; these prove the subject
    /// resolver reaches it rather than answering for itself.
    #[test]
    fn a_lone_account_is_implied_without_a_tty() {
        let only = vec![synced()];
        assert_eq!(select_account(None, &only).unwrap().email, "a@x.io");
    }

    #[test]
    fn an_omitted_subject_account_names_the_flag_without_a_tty() {
        let err = select_account(None, &accounts()).unwrap_err().to_string();
        assert!(err.contains("--account <email>"), "{err}");
        assert!(err.contains("a@x.io, b@x.io"), "{err}");
    }

    /// An Account Bichon reports nothing for is not the same question as an
    /// address it does not know, and "No accounts configured" would send the
    /// operator to auberge's own config.
    #[test]
    fn an_empty_bichon_roster_says_whose_roster_is_empty() {
        let err = select_account(None, &[]).unwrap_err().to_string();
        assert_eq!(err, "Bichon reports no accounts");
    }

    #[test]
    fn an_explicit_synced_folder_passes_through() {
        assert_eq!(
            select_synced_folder(Some("Sent".to_string()), &synced()).unwrap(),
            "Sent"
        );
    }

    /// Only a Synced Folder's coverage can be proven, so a folder outside the
    /// set is refused rather than matched against no store message and
    /// reported `covered` over nothing.
    #[test]
    fn an_explicit_folder_must_be_one_the_account_syncs() {
        let err = select_synced_folder(Some("Trash".to_string()), &synced()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Trash"), "{msg}");
        assert!(msg.contains("a@x.io"), "{msg}");
        assert!(msg.contains("INBOX, Sent"), "{msg}");
    }

    /// One guard for both arms: naming a folder does not make an Account that
    /// syncs nothing answerable. Mutation-test it by moving the `ensure!`
    /// into either arm — the other assertion then fails.
    #[test]
    fn an_account_syncing_nothing_is_refused_named_or_not() {
        let expected = "b@x.io syncs no folder, so the Email Archive can vouch for nothing — \
                        run `auberge bichon reconcile-folders --apply`";

        let named = select_synced_folder(Some("INBOX".to_string()), &accounts()[0])
            .unwrap_err()
            .to_string();
        let omitted = select_synced_folder(None, &accounts()[0])
            .unwrap_err()
            .to_string();

        assert_eq!(named, expected);
        assert_eq!(omitted, expected);
    }

    #[test]
    fn a_lone_synced_folder_is_implied_without_a_tty() {
        let account = Account {
            id: 3,
            email: "c@x.io".to_string(),
            sync_folders: vec!["INBOX".to_string()],
        };
        assert_eq!(select_synced_folder(None, &account).unwrap(), "INBOX");
    }

    /// Mutation-test the sort by reversing the fixture: the rows the error
    /// names are the rows the picker would have drawn, in one order.
    #[test]
    fn an_omitted_folder_names_the_flag_and_the_sorted_set_without_a_tty() {
        let err = select_synced_folder(None, &synced())
            .unwrap_err()
            .to_string();
        assert!(err.contains("--folder <name>"), "{err}");
        assert!(err.contains("INBOX, Sent"), "{err}");
    }

    /// The roster arrives unsorted, so the refusal's list proves the ordering
    /// is this function's job rather than each caller's.
    #[test]
    fn the_account_refusal_names_the_roster_in_one_order() {
        let err = select_account(Some("ghost@x.io".to_string()), &accounts())
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "unknown account 'ghost@x.io'; Bichon reports a@x.io, b@x.io"
        );
    }
}
