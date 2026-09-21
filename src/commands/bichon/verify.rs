use crate::commands::bichon::selection::{connect, select_account, select_synced_folder};
use crate::hosts::{HOST_FLAG, select_or_arg};
use crate::output::{self, OutputFormat};
use crate::services::bichon::api::{Account, BichonApiClient};
use crate::services::bichon::coverage::{
    CoverageReport, compare_coverage, parse_sidecar_rows, sidecar_rows_command,
    validate_archive_path_for_shell,
};
use crate::services::bichon::rescan::{sanitize_email, validate_email_for_shell};
use crate::services::ssh::{LiveSshSession, SshSession};
use chrono::{Datelike, NaiveDate};
use eyre::{Result, WrapErr};
use serde::Serialize;

pub async fn run_verify_coverage(
    host: Option<String>,
    account: Option<String>,
    folder: Option<String>,
    before: String,
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
/// `--before` has no candidates to offer, so clap keeps it required.
async fn verify_inner(
    host_arg: Option<String>,
    account_arg: Option<String>,
    folder_arg: Option<String>,
    before: String,
    archive_path: String,
    output: OutputFormat,
) -> Result<i32> {
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

    let report = compute_coverage(
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
        &report,
        output,
    )?;
    Ok(report.status().exit_code())
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
) -> Result<CoverageReport> {
    // The window is "strictly older than the cutoff date"; Bichon's `before`
    // bound is inclusive, so back off the midnight timestamp by one.
    let cutoff_ms = cutoff
        .and_hms_opt(0, 0, 0)
        .expect("midnight is a valid time")
        .and_utc()
        .timestamp_millis()
        - 1;
    let envelopes = client.search_messages(account.id, cutoff_ms).await?;

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

    compare_coverage(&envelopes, &rows, folder, (cutoff.year(), cutoff.month()))
}

#[derive(Serialize)]
struct VerifyOutputDoc<'a> {
    host: &'a str,
    account: &'a str,
    folder: &'a str,
    before: &'a str,
    status: &'static str,
    #[serde(flatten)]
    report: &'a CoverageReport,
}

fn emit_output(
    host: &str,
    account: &str,
    folder: &str,
    before: &str,
    report: &CoverageReport,
    output: OutputFormat,
) -> Result<()> {
    let status = report.status();
    match output {
        OutputFormat::Json => {
            let doc = VerifyOutputDoc {
                host,
                account,
                folder,
                before,
                status: status.as_str(),
                report,
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
            for m in &report.missing {
                println!("missing: {} (date {}, uid {})", m.message_id, m.date, m.uid);
            }
            if report.unverifiable.store_synthetic > 0 || report.unverifiable.archive_sha256 > 0 {
                println!(
                    "unverifiable by identity: {} synthetic store message(s) vs {} sha256-keyed sidecar(s)",
                    report.unverifiable.store_synthetic, report.unverifiable.archive_sha256
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

        // 2026-05-13T00:00:00Z is 1778630400000; the inclusive bound backs
        // off by one so a message dated exactly at midnight stays outside.
        Mock::given(method("POST"))
            .and(path("/api/v1/search-messages"))
            .and(body_partial_json(json!({
                "filter": {"account_ids": [7], "before": 1778630399999i64}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{
                    "id": "e1", "message_id": "a@x.io", "mailbox_name": "INBOX",
                    "uid": 1, "date": 1767225600000i64, "internal_date": 0
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
        let report = compute_coverage(
            &client,
            &ssh,
            &account(),
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap();

        assert_eq!(report.status(), CoverageStatus::Covered);
        assert_eq!(report.matched, 1);

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
                    "uid": 9, "date": 1767225600000i64, "internal_date": 0
                }],
                "total_pages": 1,
                "total_items": 1
            })))
            .mount(&server)
            .await;

        let ssh = MockSshSession::new();
        ssh.stage_run_result(stdout(""));

        let client = BichonApiClient::new(server.uri(), "token").unwrap();
        let report = compute_coverage(
            &client,
            &ssh,
            &account(),
            "INBOX",
            cutoff(),
            "/var/lib/bichon-archive",
        )
        .await
        .unwrap();

        assert_eq!(report.status(), CoverageStatus::Gap);
        assert_eq!(report.missing.len(), 1);
        assert_eq!(report.missing[0].message_id, "ghost@x.io");
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
