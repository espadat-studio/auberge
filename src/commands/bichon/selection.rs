use crate::config::Config;
use crate::hosts::Host;
use crate::prompt;
use crate::services::bichon::api::{Account, BichonApiClient};
use crate::services::bichon::derive_base_url;
use eyre::Result;

/// Reads as a meta-entry rather than an address: every other picker row is an
/// email, and `[all apps]` already sets this spelling in `deploy`.
const ALL_ACCOUNTS: &str = "[all accounts]";

/// One Host's Bichon, reached: the client the subcommand talks through, the
/// Account roster that Bichon reports, and the Config both were read out of.
///
/// `config` rides along rather than being re-loaded downstream because the
/// token and the base URL already came from it — `reconcile-folders` reads
/// its per-account exclusion overrides from the same load, so a run answers
/// out of one view of `config.toml` rather than two.
pub struct Bichon {
    pub config: Config,
    pub client: BichonApiClient,
    pub accounts: Vec<Account>,
}

/// The connection every `bichon` subcommand opens before it can ask anything:
/// the token, the base URL, the client and the roster in one place.
///
/// `rescan`, `reconcile-folders` and `verify-coverage` each held a copy of
/// these lines, and the copies had drifted in two ways that are settled here
/// rather than inherited (#934). #922 moved the Account question into this
/// module; the connection that has to happen before the question can be
/// asked now moves with it.
///
/// **An empty roster is a refusal, and it names the Host.** Every subcommand
/// acts on a Host's Bichon, so "which Bichon" belongs in the answer:
/// `reconcile-folders` used to report an empty plan and exit 0, which reads
/// as "nothing to do" when the truth is "nothing to do it to". The one
/// wording is `rescan`'s, because it was the only one that named the Host.
///
/// **The roster is ordered here**, and nowhere below. Every consumer wants
/// the same order — the rows a picker draws, the addresses a refusal lists,
/// the accounts a plan walks — so sorting once at the source is cheaper than
/// each of them re-stating it and drifting. [`select_account`] and
/// [`unknown_account`] trust this rather than re-sorting.
pub async fn connect(host: &Host) -> Result<Bichon> {
    let config = Config::load()?;
    let token = config
        .get_resolved("bichon_api_token")?
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| eyre::eyre!("bichon_api_token not set in config.toml"))?;
    let client = BichonApiClient::new(derive_base_url(&config, host)?, token)?;

    let mut accounts = client.list_accounts().await?;
    accounts.sort_by(|a, b| a.email.cmp(&b.email));
    eyre::ensure!(
        !accounts.is_empty(),
        "Bichon reports no accounts on '{}'",
        host.name
    );

    Ok(Bichon {
        config,
        client,
        accounts,
    })
}

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
        // A picker over `[all accounts]` alone is not a question. No caller
        // reaches this arm through [`connect`], which refuses an empty
        // roster first; it holds the line for a roster narrowed to nothing
        // by anything else.
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
/// The list is named in the order it arrives, which [`connect`] has already
/// set: the addresses a refusal prints are the rows a picker would have
/// drawn because both read the same roster, not because both sort it.
fn unknown_account(email: &str, known: &[String]) -> eyre::Report {
    let reported = if known.is_empty() {
        "no accounts".to_string()
    } else {
        known.join(", ")
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
/// `accounts` is [`connect`]'s roster: non-empty and ordered by email. Both
/// are its entry condition rather than this function's job — an empty roster
/// is refused there, naming the Host, which is a thing only the caller of
/// `connect` knows.
pub fn select_account(arg: Option<String>, accounts: &[Account]) -> Result<Account> {
    match arg {
        Some(email) => accounts
            .iter()
            .find(|a| a.email == email)
            .cloned()
            .ok_or_else(|| {
                let known: Vec<String> = accounts.iter().map(|a| a.email.clone()).collect();
                unknown_account(&email, &known)
            }),
        None => prompt::select_item(
            accounts,
            |a: &Account| a.email.clone(),
            prompt::Choice::new("account").resolved_by("--account <email>"),
        ),
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
/// The set is sorted here rather than trusted from the API: [`connect`]
/// orders the roster, not the folders inside each Account.
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

/// The config a `bichon` test runs [`connect`] against, written once.
///
/// It lives here rather than in a test module of its own because every
/// `bichon` command reaches Bichon through `connect`, so every `bichon`
/// command's tests need the same Host, token and base URL to reach it — the
/// same argument that collapsed the three copies of `connect` itself.
#[cfg(test)]
pub(super) mod test_support {
    use crate::output::EnvVarGuard;
    use eyre::Result;
    use std::fs;
    use tempfile::TempDir;
    use wiremock::MockServer;

    /// Keeps the temp tree and the env guards alive for the test's length;
    /// dropping it restores the previous `XDG_*` values.
    pub(crate) struct TestEnv {
        _tmp: TempDir,
        _xdg_config: EnvVarGuard,
        _xdg_data: EnvVarGuard,
    }

    /// One Host named `auberge`, a token, and a per-host base URL pointed at
    /// `server`.
    ///
    /// Caller MUST hold [`crate::output::TEST_LOCK`]: the guards mutate
    /// process env.
    pub(crate) fn prepare_config(server: &MockServer) -> Result<TestEnv> {
        let tmp = tempfile::tempdir()?;
        let config_home = tmp.path().join("cfg");
        let data_home = tmp.path().join("data");
        fs::create_dir_all(config_home.join("auberge"))?;
        fs::create_dir_all(&data_home)?;
        fs::write(
            config_home.join("auberge/config.toml"),
            format!(
                r#"
domain = "example.com"
bichon_api_token = "token-123"
[bichon.hosts.auberge]
base_url = "{}"
[bichon.account_overrides."me@sripwoud.xyz"]
extra_excluded_folders = ["Receipts/2019"]
"#,
                server.uri()
            ),
        )?;
        fs::write(
            config_home.join("auberge/hosts.toml"),
            r#"
[[hosts]]
name = "auberge"
address = "100.100.100.10"
user = "root"
tailscale_ip = "100.100.100.10"
"#,
        )?;

        let xdg_config = EnvVarGuard::set("XDG_CONFIG_HOME", &config_home);
        let xdg_data = EnvVarGuard::set("XDG_DATA_HOME", &data_home);
        Ok(TestEnv {
            _tmp: tmp,
            _xdg_config: xdg_config,
            _xdg_data: xdg_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::prepare_config;
    use super::*;
    use crate::hosts::HostManager;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    /// `connect`'s roster, as the resolvers below receive it: ordered by
    /// email, which is why nothing below sorts again.
    fn accounts() -> Vec<Account> {
        vec![
            Account {
                id: 1,
                email: "a@x.io".to_string(),
                sync_folders: vec!["Sent".to_string(), "INBOX".to_string()],
            },
            Account {
                id: 2,
                email: "b@x.io".to_string(),
                sync_folders: Vec::new(),
            },
        ]
    }

    /// The Account with two Synced Folders, deliberately unsorted so the
    /// folder assertions below have something to prove.
    fn synced() -> Account {
        accounts().remove(0)
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
        assert_eq!(
            err.to_string(),
            "unknown account 'ghost@x.io'; Bichon reports a@x.io, b@x.io"
        );
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

        let named = select_synced_folder(Some("INBOX".to_string()), &accounts()[1])
            .unwrap_err()
            .to_string();
        let omitted = select_synced_folder(None, &accounts()[1])
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

    fn host() -> Host {
        HostManager::get_host("auberge").unwrap()
    }

    async fn mount_roster(server: &MockServer, items: serde_json::Value) {
        let total = items.as_array().map_or(0, Vec::len);
        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"items": items, "total_items": total})),
            )
            .mount(server)
            .await;
    }

    /// The one place the roster's order is stated. Bichon answers unsorted —
    /// mutation-test by dropping the `sort_by` in `connect` and this fails
    /// while every resolver below it keeps passing, which is the point: they
    /// no longer re-state it.
    #[tokio::test]
    async fn the_roster_arrives_ordered_by_email() {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server).unwrap();
        mount_roster(
            &server,
            json!([
                {"id": 2, "email": "b@x.io", "sync_folders": []},
                {"id": 1, "email": "a@x.io", "sync_folders": []}
            ]),
        )
        .await;

        let bichon = connect(&host()).await.unwrap();
        let emails: Vec<&str> = bichon.accounts.iter().map(|a| a.email.as_str()).collect();
        assert_eq!(emails, ["a@x.io", "b@x.io"]);
    }

    /// The one wording for an empty roster, shared by all three subcommands
    /// because all three reach Bichon here. It names the Host: `rescan` was
    /// the only copy that did, `verify-coverage` said it without one and
    /// `reconcile-folders` said nothing and exited 0 (#934).
    #[tokio::test]
    async fn an_empty_roster_is_refused_and_names_the_host() {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server).unwrap();
        mount_roster(&server, json!([])).await;

        // `.err()` rather than `unwrap_err()`: the Ok side carries the API
        // token, and `Debug` on it would print the token on a failure.
        let err = connect(&host())
            .await
            .err()
            .expect("an empty roster must be refused");
        assert_eq!(err.to_string(), "Bichon reports no accounts on 'auberge'");
    }
}
