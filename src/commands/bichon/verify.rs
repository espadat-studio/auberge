use crate::commands::bichon::selection::{connect, select_account, select_synced_folder};
use crate::hosts::{HOST_FLAG, select_or_arg};
use crate::output::{self, OutputFormat};
use crate::services::bichon::api::{Account, BichonApiClient};
use crate::services::bichon::coverage::{
    CoverageReport, StoreCount, compare_coverage, count_store_messages, parse_sidecar_rows,
    sidecar_rows_command, validate_archive_path_for_shell,
};
use crate::services::bichon::rescan::{sanitize_email, validate_email_for_shell};
use crate::services::ssh::{LiveSshSession, SshSession};
use chrono::NaiveDate;
use eyre::{Result, WrapErr};
use serde::Serialize;

pub async fn run_verify_coverage(
    host: Option<String>,
    account: Option<String>,
    folder: Option<String>,
    before: Option<String>,
    archive_path: String,
    output: OutputFormat,
) -> Result<i32> {
    match verify_inner(host, account, folder, before, archive_path, output).await {
        Ok(code) => Ok(code),
        Err(err) => {
            output::warn(&format!("verify-coverage failed: {err:#}"));
            Ok(2)
        }
    }
}

/// All three subjects are resolved here, before the walk that needs them:
/// the Host through `select_or_arg`, the Account and the Folder through the
/// `selection` pair. Each picker's candidates come from the answer above it —
/// the Account roster from the Host's Bichon, the Synced Folders from the
/// Account — which is why `connect` runs between them rather than after.
///
/// `--before` has no candidates to offer, so it is a free-text prompt.
async fn verify_inner(
    host_arg: Option<String>,
    account_arg: Option<String>,
    folder_arg: Option<String>,
    before: Option<String>,
    archive_path: String,
    output: OutputFormat,
) -> Result<i32> {
    let before = crate::prompt::text_or_arg(
        before,
        "Verify messages before (YYYY-MM-DD, UTC)",
        "--before",
    )?;
    let cutoff = NaiveDate::parse_from_str(&before, "%Y-%m-%d")
        .wrap_err_with(|| format!("--before must be a YYYY-MM-DD date, got '{before}'"))?;
    validate_archive_path_for_shell(&archive_path)?;

    let host = select_or_arg(host_arg, HOST_FLAG)?;
    let bichon = connect(&host).await?;

    let account = select_account(account_arg, &bichon.accounts)?;
    validate_email_for_shell(&account.email)?;
    let folder = select_synced_folder(folder_arg, &account)?;

    let ssh_key = crate::services::ssh::resolve_ssh_key_path(&host, None)?;
    let route = crate::services::route::resolve(&host, Some(ssh_key))?;
    let ssh = LiveSshSession::new(&route, &host.become_method)?;

    let verdict = compute_coverage(
        &bichon.client,
        &ssh,
        &account,
        &folder,
        cutoff,
        &archive_path,
    )
    .await?;
    emit_output(
        &host.name,
        &account.email,
        &folder,
        &before,
        &verdict,
        output,
    )?;
    Ok(verdict.report.status().exit_code())
}

/// Both halves of Expunge Coverage the store can answer (ADR-0088): the
/// folder's raw store count on the internal date, which the expunge script
/// bounds its IMAP count by, and the identity report against the archive.
#[derive(Serialize, Debug)]
struct Verdict {
    #[serde(flatten)]
    store_count: StoreCount,
    #[serde(flatten)]
    report: CoverageReport,
}

/// The whole verdict behind the seams the tests can reach: the Bichon API
/// (wiremock) and the Host walk (`SshSession`).
async fn compute_coverage(
    client: &BichonApiClient,
    ssh: &dyn SshSession,
    account: &Account,
    folder: &str,
    cutoff: NaiveDate,
    archive_path: &str,
) -> Result<Verdict> {
    let envelopes = client.search_messages(account.id).await?;
    let store_count = count_store_messages(&envelopes, folder, cutoff)?;

    let archive_dir = format!(
        "{}/{}",
        archive_path.trim_end_matches('/'),
        sanitize_email(&account.email)
    );
    let walk = ssh.run(&sidecar_rows_command(&archive_dir))?;
    if !walk.success {
        if walk.exit_code == Some(3) {
            eyre::bail!(
                "no Email Archive directory at {archive_dir}; the archive can vouch for nothing"
            );
        }
        eyre::bail!(
            "could not read the archived sidecars under {archive_dir}: {}",
            walk.stderr_str().trim()
        );
    }
    let rows = parse_sidecar_rows(&walk.stdout_str())?;

    let report = compare_coverage(&envelopes, &rows, folder, cutoff)?;
    Ok(Verdict {
        store_count,
        report,
    })
}

#[derive(Serialize)]
struct VerifyOutputDoc<'a> {
    host: &'a str,
    account: &'a str,
    folder: &'a str,
    before: &'a str,
    status: &'static str,
    #[serde(flatten)]
    verdict: &'a Verdict,
}

fn emit_output(
    host: &str,
    account: &str,
    folder: &str,
    before: &str,
    verdict: &Verdict,
    output: OutputFormat,
) -> Result<()> {
    let report = &verdict.report;
    let status = report.status();
    match output {
        OutputFormat::Json => {
            let doc = VerifyOutputDoc {
                host,
                account,
                folder,
                before,
                status: status.as_str(),
                verdict,
            };
            let json = serde_json::to_string_pretty(&doc)
                .wrap_err("failed to serialize verify-coverage output as JSON")?;
            println!("{json}");
        }
        OutputFormat::Human => {
            println!(
                "{account} {folder} before {before}: {} store message(s), {} matched, {} missing",
                report.store_messages,
                report.matched,
                report.missing.len()
            );
            println!(
                "{} store message(s) by internal date, the bound for the IMAP count ({} without one)",
                verdict.store_count.before_cutoff,
                verdict.store_count.without_internal_date.len()
            );
            for m in &report.missing {
                println!("missing: {} (date {}, uid {})", m.message_id, m.date, m.uid);
            }
            if report.unverifiable.store_synthetic > 0 || report.unverifiable.archive_sha256 > 0 {
                println!(
                    "unverifiable by identity: {} synthetic store message(s) vs {} sha256-keyed sidecar(s)",
                    report.unverifiable.store_synthetic, report.unverifiable.archive_sha256
                );
            }
            for c in &report.unverifiable.contested {
                println!(
                    "contested: {} claimed by {} store message(s) vs {} archived body(ies)",
                    c.message_id, c.store, c.archive
                );
            }
            println!("status: {}", status.as_str());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compute_coverage;
    use crate::services::bichon::api::{Account, BichonApiClient};
    use crate::services::bichon::coverage::CoverageStatus;
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};
    use chrono::NaiveDate;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cutoff() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 13).unwrap()
    }

    /// The Account arrives resolved: `select_account` answered "which
    /// account", so the verdict no longer re-asks the API for the roster and
    /// no longer owns an unknown-address error of its own.
    fn account() -> Account {
        Account {
            id: 7,
            email: "me@x.io".to_string(),
            sync_folders: vec!["INBOX".to_string()],
        }
    }

    fn stdout(text: &str) -> CommandResult {
        CommandResult {
            success: true,
            exit_code: Some(0),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_covered_folder_reports_covered() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .and(body_partial_json(json!({"filter": {"account_ids": [7]}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{
                    "id": "e1", "message_id": "a@x.io", "mailbox_name": "INBOX",
                    "uid": 1, "date": 1767225600000i64, "internal_date": 1767225600000i64
                }],
                "total_pages": 1,
                "total_items": 1
            })))
            .mount(&server)
            .await;

        let ssh = MockSshSession::new();
        ssh.stage_run_result(stdout(
            "/var/lib/bichon-archive/me@x.io/2026/01/1.meta.json\tINBOX\ta@x.io\n",
        ));

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let verdict = compute_coverage(
            &client,
            &ssh,
            &account(),
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap();

        assert_eq!(verdict.report.status(), CoverageStatus::Covered);
        assert_eq!(verdict.report.matched, 1);

        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["filter"], json!({"account_ids": [7]}));

        let calls = ssh.calls();
        assert_eq!(calls.len(), 1);
        let SshOp::Run(cmd) = &calls[0] else {
            panic!("expected a run call");
        };
        assert!(cmd.contains("/var/lib/bichon-archive/me@x.io"));
    }

    #[tokio::test]
    async fn a_store_message_the_archive_lacks_is_a_gap() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{
                    "id": "e1", "message_id": "ghost@x.io", "mailbox_name": "INBOX",
                    "uid": 9, "date": 1767225600000i64, "internal_date": 1767225600000i64
                }],
                "total_pages": 1,
                "total_items": 1
            })))
            .mount(&server)
            .await;

        let ssh = MockSshSession::new();
        ssh.stage_run_result(stdout(""));

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let verdict = compute_coverage(
            &client,
            &ssh,
            &account(),
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap();

        assert_eq!(verdict.report.status(), CoverageStatus::Gap);
        assert_eq!(verdict.report.missing.len(), 1);
        assert_eq!(verdict.report.missing[0].message_id, "ghost@x.io");
    }

    // 2026-05-13T00:00:00Z; the IMAP side asks `SEARCH BEFORE 2026-05-13`.
    const MIDNIGHT: i64 = 1_778_630_400_000;

    async fn store_of(items: serde_json::Value) -> MockServer {
        let server = MockServer::start().await;
        let total = items.as_array().unwrap().len();
        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": items,
                "total_pages": 1,
                "total_items": total
            })))
            .mount(&server)
            .await;
        server
    }

    async fn verdict_of(server: &MockServer, sidecars: &str) -> super::Verdict {
        let ssh = MockSshSession::new();
        ssh.stage_run_result(stdout(sidecars));
        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        compute_coverage(
            &client,
            &ssh,
            &account(),
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap()
    }

    // The count gate's store side is a raw count on the IMAP internal date,
    // the axis `SEARCH BEFORE` uses, so a message whose Date header and
    // internal date fall either side of the cutoff is counted by the latter.
    #[tokio::test]
    async fn the_store_count_is_taken_on_the_internal_date_before_the_cutoff() {
        let server = store_of(json!([
            {"message_id": "edge@x.io", "mailbox_name": "INBOX", "uid": 1,
             "date": MIDNIGHT - 1, "internal_date": MIDNIGHT - 1},
            {"message_id": "midnight@x.io", "mailbox_name": "INBOX", "uid": 2,
             "date": MIDNIGHT - 1, "internal_date": MIDNIGHT},
            {"message_id": "backdated@x.io", "mailbox_name": "INBOX", "uid": 3,
             "date": MIDNIGHT + 86_400_000, "internal_date": MIDNIGHT - 86_400_000},
            {"message_id": "elsewhere@x.io", "mailbox_name": "Sent", "uid": 4,
             "date": MIDNIGHT - 1, "internal_date": MIDNIGHT - 1}
        ]))
        .await;

        let verdict = verdict_of(
            &server,
            "/a/2026/05/1.meta.json\tINBOX\tedge@x.io\n/a/2026/05/2.meta.json\tINBOX\tmidnight@x.io\n",
        )
        .await;

        assert_eq!(verdict.store_count.before_cutoff, 2);
        assert!(verdict.store_count.without_internal_date.is_empty());
        assert_eq!(verdict.report.store_messages, 2);
    }

    // A folder holding one Message-ID twice is two messages on the server and
    // two in the store; the count compares raw messages, never distinct ids.
    #[tokio::test]
    async fn repeated_message_ids_are_counted_once_each() {
        let server = store_of(json!([
            {"message_id": "dup@x.io", "mailbox_name": "INBOX", "uid": 1,
             "date": 0, "internal_date": MIDNIGHT - 2},
            {"message_id": "dup@x.io", "mailbox_name": "INBOX", "uid": 2,
             "date": 0, "internal_date": MIDNIGHT - 1}
        ]))
        .await;

        let verdict = verdict_of(
            &server,
            "/a/2026/01/1.meta.json\tINBOX\tdup@x.io\n/a/2026/01/2.meta.json\tINBOX\tdup@x.io\n",
        )
        .await;

        assert_eq!(verdict.store_count.before_cutoff, 2);
        assert_eq!(verdict.report.status(), CoverageStatus::Covered);
    }

    // me@ Archive uid 648 on auberge: MXroute's quota notice, no Date and no
    // Message-ID, received 2025-03-10 per IMAP, stored by Bichon with
    // internal_date 0. Bichon's own filter counts 0 inside every window;
    // the verdict does too, and names their uids so the expunge script can
    // check each one against the uids its own IMAP SEARCH BEFORE returned.
    #[tokio::test]
    async fn a_store_message_without_an_internal_date_is_counted_and_reported() {
        let server = store_of(json!([
            {"message_id": "a@x.io", "mailbox_name": "INBOX", "uid": 1,
             "date": 0, "internal_date": MIDNIGHT - 1},
            {"message_id": "<0f.1.2@bichon>", "mailbox_name": "INBOX", "uid": 648,
             "date": 0, "internal_date": 0}
        ]))
        .await;

        let verdict = verdict_of(
            &server,
            "/a/2026/01/1.meta.json\tINBOX\ta@x.io\n/a/2026/01/2.meta.json\tINBOX\tsha256:aa\n",
        )
        .await;

        assert_eq!(verdict.store_count.before_cutoff, 2);
        assert_eq!(verdict.store_count.without_internal_date, vec![648]);
        let json = serde_json::to_value(&verdict).unwrap();
        assert_eq!(json["store_internal_date_count"], 2);
        assert_eq!(json["store_without_internal_date"], json!([648]));
    }

    #[tokio::test]
    async fn a_missing_archive_directory_is_an_operational_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [],
                "total_pages": 0,
                "total_items": 0
            })))
            .mount(&server)
            .await;

        let ssh = MockSshSession::new();
        ssh.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(3),
            stdout: Vec::new(),
            stderr: Vec::new(),
        });

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let err = compute_coverage(
            &client,
            &ssh,
            &account(),
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap_err();

        assert!(format!("{err}").contains("no Email Archive directory"));
    }
}
