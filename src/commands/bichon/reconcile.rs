use crate::commands::bichon::selection::{BichonConnection, connect, resolve_account_filter};
use crate::hosts::{HOST_FLAG, select_or_arg};
use crate::output::{self, OutputFormat};
use crate::services::bichon::api::Account;
use crate::services::bichon::folder_filter::is_excluded;
use eyre::{Result, WrapErr};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct AccountPlan {
    pub email: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: Vec<String>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct ReconcileSummary {
    pub added: usize,
    pub removed: usize,
    pub changed_accounts: usize,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct ReconcileOutput {
    pub host: String,
    pub apply: bool,
    pub account: Option<String>,
    pub accounts: Vec<AccountPlan>,
    pub summary: ReconcileSummary,
}

pub async fn run_reconcile_folders(
    host: Option<String>,
    apply: bool,
    account_filter: Option<String>,
    output: OutputFormat,
) -> Result<()> {
    let result = compute_reconcile(host, apply, account_filter).await?;
    emit_output(&result, output)?;
    if result.apply && result.summary.added > 0 {
        let account_flag = result
            .account
            .as_ref()
            .map(|a| format!(" --account {a}"))
            .unwrap_or_default();
        output::warn(&format!(
            "added {} folder(s); mail already in them with a Date behind the archive cursor is never archived by the hourly run — run `auberge bichon rescan --host {}{}` to backfill",
            result.summary.added, result.host, account_flag
        ));
    }
    Ok(())
}

/// `-H` and `--account` are both resolved here rather than filtered later,
/// through the same pair `rescan` uses: `select_or_arg` for the Host it acts
/// on, [`resolve_account_filter`] for the listing it narrows. The policy
/// behind each lives on those two, not here.
///
/// The plan walks the roster in the order [`connect`] set, so a Host's plan
/// reads the same way twice running whatever order Bichon answered in.
pub async fn compute_reconcile(
    host_arg: Option<String>,
    apply: bool,
    account_filter: Option<String>,
) -> Result<ReconcileOutput> {
    let host_record = select_or_arg(host_arg, HOST_FLAG)?;
    let host = host_record.name.clone();

    let BichonConnection {
        config,
        client,
        accounts,
    } = connect(&host_record).await?;

    let known: Vec<String> = accounts.iter().map(|a| a.email.clone()).collect();
    let account_filter =
        resolve_account_filter(account_filter, &known, crate::prompt::is_interactive())?;
    let accounts: Vec<Account> = match &account_filter {
        Some(email) => accounts.into_iter().filter(|a| &a.email == email).collect(),
        None => accounts,
    };

    let mut plans = Vec::new();
    let mut total_added = 0usize;
    let mut total_removed = 0usize;
    let mut changed_accounts = 0usize;

    for account in accounts {
        let mailbox_list = client.list_mailboxes(account.id).await?;
        let extra_excluded: HashSet<String> = config
            .bichon_extra_excluded_folders(&account.email)
            .into_iter()
            .collect();

        let desired_set: HashSet<String> = mailbox_list
            .iter()
            .filter(|mb| !is_excluded(mb, &extra_excluded))
            .map(|mb| mb.name.clone())
            .collect();
        let current_set: HashSet<String> = account.sync_folders.iter().cloned().collect();

        let mut added: Vec<String> = desired_set.difference(&current_set).cloned().collect();
        let mut removed: Vec<String> = current_set.difference(&desired_set).cloned().collect();
        let mut unchanged: Vec<String> = current_set.intersection(&desired_set).cloned().collect();
        let mut desired_sorted: Vec<String> = desired_set.into_iter().collect();
        added.sort();
        removed.sort();
        unchanged.sort();
        desired_sorted.sort();

        if !added.is_empty() || !removed.is_empty() {
            changed_accounts += 1;
            total_added += added.len();
            total_removed += removed.len();
            if apply {
                client
                    .update_account_sync_folders(account.id, &desired_sorted)
                    .await
                    .wrap_err_with(|| {
                        format!("failed to update sync_folders for {}", account.email)
                    })?;
            }
        }

        plans.push(AccountPlan {
            email: account.email,
            added,
            removed,
            unchanged,
        });
    }

    Ok(ReconcileOutput {
        host,
        apply,
        account: account_filter,
        accounts: plans,
        summary: ReconcileSummary {
            added: total_added,
            removed: total_removed,
            changed_accounts,
        },
    })
}

fn emit_output(result: &ReconcileOutput, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(result)
                .wrap_err("failed to serialize reconcile output as JSON")?;
            println!("{json}");
        }
        OutputFormat::Human => {
            for account in &result.accounts {
                println!("{}:", account.email);
                if !account.added.is_empty() {
                    println!("  + Add to sync_folders: {:?}", account.added);
                }
                if !account.removed.is_empty() {
                    println!("  - Remove from sync_folders: {:?}", account.removed);
                }
                println!("  unchanged: {:?}", account.unchanged);
            }
            if result.apply {
                println!(
                    "\nApplied: {} added, {} removed across {} account(s).",
                    result.summary.added, result.summary.removed, result.summary.changed_accounts
                );
            } else {
                println!(
                    "\nPlan: {} added, {} removed across {} account(s). Run with --apply to commit.",
                    result.summary.added, result.summary.removed, result.summary.changed_accounts
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ReconcileSummary, compute_reconcile};
    use crate::commands::bichon::selection::test_support::prepare_config;
    use crate::output::EnvVarGuard;
    use eyre::Result;
    use serde_json::json;
    use std::fs;
    use wiremock::matchers::{body_json, method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// One Account whose `sync_folders` already match the mailboxes Bichon
    /// lists, so a test that is about something else — host resolution, a
    /// retry, a base URL — gets a roster `connect` accepts and a plan with
    /// no diff in it.
    async fn mount_one_account_in_sync(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{"id":1, "email":"me@sripwoud.xyz", "sync_folders":["INBOX"]}],
                "total_items": 1
            })))
            .mount(server)
            .await;
        mount_mailboxes(server).await;
    }

    async fn mount_mailboxes(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/api/v1/list-mailboxes/1"))
            .and(query_param("remote", "true"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([{"name":"INBOX","attributes":[]}])),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn dry_run_returns_expected_diff() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server)?;

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [
                    {"id":1, "email":"me@sripwoud.xyz", "sync_folders":["INBOX","Sent","INBOX/old-archive","Receipts/2019"]},
                    {"id":2, "email":"work@sripwoud.xyz", "sync_folders":["INBOX","Sent"]}
                ],
                "total_items": 2
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v1/list-mailboxes/1"))
            .and(query_param("remote", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"name":"INBOX","attributes":[]},
                {"name":"Sent","attributes":[]},
                {"name":"INBOX/legal-2026","attributes":[]},
                {"name":"INBOX/old-archive","attributes":[{"attr":"Trash"}]},
                {"name":"Receipts/2019","attributes":[]}
            ])))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path_regex(r"^/api/v1/account/\d+$"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        let result = compute_reconcile(
            Some("auberge".to_string()),
            false,
            Some("me@sripwoud.xyz".to_string()),
        )
        .await?;

        assert!(!result.apply);
        assert_eq!(result.host, "auberge");
        assert_eq!(result.account.as_deref(), Some("me@sripwoud.xyz"));
        assert_eq!(result.accounts.len(), 1);
        let plan = &result.accounts[0];
        assert_eq!(plan.email, "me@sripwoud.xyz");
        assert_eq!(plan.added, vec!["INBOX/legal-2026".to_string()]);
        // excluded by SPECIAL-USE \Trash and by extra_excluded_folders override
        let mut removed = plan.removed.clone();
        removed.sort();
        assert_eq!(
            removed,
            vec!["INBOX/old-archive".to_string(), "Receipts/2019".to_string()]
        );
        assert_eq!(
            plan.unchanged,
            vec!["INBOX".to_string(), "Sent".to_string()]
        );
        assert_eq!(
            result.summary,
            ReconcileSummary {
                added: 1,
                removed: 2,
                changed_accounts: 1
            }
        );
        Ok(())
    }

    /// The bug #922 was filed for. Before the fix `--account` was a plain
    /// equality match over the listing, so an address Bichon does not report
    /// filtered every Account away and the run reported
    /// `0 folders added across 0 accounts` as a success. Mutation-test it by
    /// restoring the `.filter()`: this returns `Ok` with an empty plan and
    /// the assertion below fails.
    #[tokio::test]
    async fn an_unknown_account_errors_instead_of_reconciling_nothing() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server)?;

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [
                    {"id":1, "email":"me@sripwoud.xyz", "sync_folders":["INBOX"]}
                ],
                "total_items": 1
            })))
            .mount(&server)
            .await;

        let err = compute_reconcile(
            Some("auberge".to_string()),
            false,
            Some("me@exmaple.com".to_string()),
        )
        .await
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(msg.contains("me@exmaple.com"), "{msg}");
        assert!(msg.contains("me@sripwoud.xyz"), "{msg}");
        Ok(())
    }

    /// `cargo test` runs without a TTY, so this exercises the scripted path:
    /// a lone configured Host is implied rather than refused. Mutation-test
    /// it by reinstating an `is_tty` guard ahead of `select_or_arg`.
    #[tokio::test]
    async fn an_omitted_host_resolves_the_lone_configured_one_off_a_tty() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server)?;
        mount_one_account_in_sync(&server).await;

        let result = compute_reconcile(None, false, None).await?;

        assert_eq!(result.host, "auberge");
        assert_eq!(result.account, None);
        Ok(())
    }

    /// An empty roster is `connect`'s refusal now, not an empty plan and
    /// exit 0: there is nothing to reconcile *and* nothing to reconcile it
    /// against, and the message says which Host answered that way (#934).
    /// Mutation-test it by dropping the `ensure!` in `connect`.
    #[tokio::test]
    async fn an_empty_roster_refuses_rather_than_planning_nothing() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server)?;

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [],
                "total_items": 0
            })))
            .mount(&server)
            .await;

        let err = compute_reconcile(Some("auberge".to_string()), false, None)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Bichon reports no accounts on 'auberge'");
        Ok(())
    }

    // v2 (bichon >= 2.0) response shapes: accounts expose download_folders,
    // list-mailboxes wraps the array in a cache envelope. The write payload
    // must still say sync_folders.
    #[tokio::test]
    async fn apply_patches_sync_folders_once_when_changed() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server)?;

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [
                    {"id":1, "email":"me@sripwoud.xyz", "download_folders":["INBOX","INBOX/old-archive"]}
                ],
                "total_items": 1
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v1/list-mailboxes/1"))
            .and(query_param("remote", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "mailboxes": [
                    {"name":"INBOX","attributes":[]},
                    {"name":"INBOX/legal-2026","attributes":[]},
                    {"name":"INBOX/old-archive","attributes":[{"attr":"Trash"}]}
                ],
                "status": "ready",
                "examined": null,
                "total": null
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/v1/account/1"))
            .and(body_json(
                json!({"sync_folders":["INBOX","INBOX/legal-2026"]}),
            ))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let result = compute_reconcile(Some("auberge".to_string()), true, None).await?;
        assert!(result.apply);
        assert_eq!(result.summary.changed_accounts, 1);
        assert_eq!(result.summary.added, 1);
        assert_eq!(result.summary.removed, 1);
        Ok(())
    }

    #[tokio::test]
    async fn apply_is_idempotent_with_no_diff() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server)?;

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [
                    {"id":1, "email":"me@sripwoud.xyz", "sync_folders":["INBOX","INBOX/legal-2026"]}
                ],
                "total_items": 1
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v1/list-mailboxes/1$"))
            .and(query_param("remote", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"name":"INBOX","attributes":[]},
                {"name":"INBOX/legal-2026","attributes":[]}
            ])))
            .mount(&server)
            .await;

        // Critical: assert NO POST is sent when there is no diff.
        Mock::given(method("POST"))
            .and(path_regex(r"^/api/v1/account/\d+$"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        let result = compute_reconcile(Some("auberge".to_string()), true, None).await?;
        assert!(result.apply);
        assert_eq!(
            result.summary,
            ReconcileSummary {
                added: 0,
                removed: 0,
                changed_accounts: 0
            }
        );
        let plan = &result.accounts[0];
        assert!(plan.added.is_empty());
        assert!(plan.removed.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn retries_on_5xx_then_succeeds() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server)?;

        // First call: 500. Second call: success.
        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        mount_one_account_in_sync(&server).await;

        let result = compute_reconcile(Some("auberge".to_string()), false, None).await?;
        assert_eq!(result.accounts.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn unknown_host_fails_before_network() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        let _env = prepare_config(&server)?;

        let err = compute_reconcile(Some("not-a-host".to_string()), false, None)
            .await
            .unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("not-a-host"),
            "expected error to mention host name, got: {msg}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn falls_back_to_global_bichon_base_url_when_per_host_missing() -> Result<()> {
        let _guard = crate::output::TEST_LOCK.lock().unwrap();
        let server = MockServer::start().await;
        // Writes its own config rather than reusing `prepare_config`: the
        // per-host `[bichon.hosts.auberge] base_url` that helper sets is the
        // very thing whose absence this test is about.
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
bichon_base_url = "{}"
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
"#,
        )?;
        let _xdg_config = EnvVarGuard::set("XDG_CONFIG_HOME", &config_home);
        let _xdg_data = EnvVarGuard::set("XDG_DATA_HOME", &data_home);

        Mock::given(method("GET"))
            .and(path("/api/v1/accounts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{"id":1, "email":"me@sripwoud.xyz", "sync_folders":["INBOX"]}],
                "total_items": 1
            })))
            .expect(1)
            .mount(&server)
            .await;
        mount_mailboxes(&server).await;

        let result = compute_reconcile(Some("auberge".to_string()), false, None).await?;
        assert_eq!(result.accounts[0].email, "me@sripwoud.xyz");
        Ok(())
    }
}
