use crate::commands::bichon::selection::{connect, resolve_account_filter};
use crate::hosts::{HOST_FLAG, select_or_arg};
use crate::output::{self, OutputFormat};
use crate::services::bichon::rescan::{
    ARCHIVE_SERVICE, AccountReport, RescanOutcome, RescanRun, execute_rescan,
};
use crate::services::ssh::LiveSshSession;
use eyre::{Result, WrapErr};
use serde::Serialize;

pub async fn run_rescan(
    host_arg: Option<String>,
    account: Option<String>,
    output: OutputFormat,
) -> Result<i32> {
    match rescan_inner(host_arg, account, output).await {
        Ok(code) => Ok(code),
        Err(err) => {
            output::warn(&format!("rescan failed: {err:#}"));
            Ok(2)
        }
    }
}

async fn rescan_inner(
    host_arg: Option<String>,
    account_filter: Option<String>,
    output: OutputFormat,
) -> Result<i32> {
    let host = select_or_arg(host_arg, HOST_FLAG)?;

    // The roster is all this command wants from Bichon: the rescan itself
    // runs over SSH. Ordered and non-empty by `connect`'s contract, so the
    // cursor reset walks the accounts in the order the picker offered them.
    let known: Vec<String> = connect(&host)
        .await?
        .accounts
        .into_iter()
        .map(|a| a.email)
        .collect();

    let account = resolve_account_filter(account_filter, &known, crate::prompt::is_interactive())?;
    let selected = match &account {
        Some(email) => vec![email.clone()],
        None => known.clone(),
    };

    let ssh_key = crate::services::ssh::resolve_ssh_key_path(&host, None)?;
    let route = crate::services::route::resolve(&host, Some(ssh_key))?;
    let ssh = LiveSshSession::new(&route, &host.become_method)?;

    output::info(&format!(
        "resetting {} archive cursor(s) on {} and starting {} — this waits for the full pass",
        selected.len(),
        host.name,
        ARCHIVE_SERVICE
    ));

    match execute_rescan(&ssh, &selected, &known)? {
        RescanOutcome::Refused { stale_sidecars } => {
            output::warn(&format!(
                "refusing to rescan: {} sidecar(s) lack message_id (e.g. {}). Deploy the current bichon role and let the hourly archive run backfill them (or start {} once), then retry.",
                stale_sidecars.len(),
                stale_sidecars[0],
                ARCHIVE_SERVICE
            ));
            Ok(2)
        }
        RescanOutcome::Busy => {
            output::warn(&format!(
                "an archive run is already in progress; retry once {ARCHIVE_SERVICE} is inactive"
            ));
            Ok(2)
        }
        RescanOutcome::Ran(run) => {
            emit_output(&host.name, account.as_deref(), &run, output)?;
            Ok(run.status().exit_code())
        }
    }
}

#[derive(Serialize)]
struct RescanOutputDoc<'a> {
    host: &'a str,
    account: Option<&'a str>,
    status: &'static str,
    accounts: &'a [AccountReport],
    total_failures: Option<u64>,
}

fn emit_output(
    host: &str,
    account: Option<&str>,
    run: &RescanRun,
    output: OutputFormat,
) -> Result<()> {
    let status = run.status();
    match output {
        OutputFormat::Json => {
            let doc = RescanOutputDoc {
                host,
                account,
                status: status.as_str(),
                accounts: &run.accounts,
                total_failures: run.total_failures,
            };
            let json = serde_json::to_string_pretty(&doc)
                .wrap_err("failed to serialize rescan output as JSON")?;
            println!("{json}");
        }
        OutputFormat::Human => {
            for a in &run.accounts {
                println!(
                    "{}: {} processed={} skipped={} failures={}",
                    a.email,
                    if a.cursor_reset {
                        "cursor reset,"
                    } else {
                        "cursor kept,"
                    },
                    a.processed,
                    a.skipped,
                    a.failures
                );
            }
            let processed: u64 = run.accounts.iter().map(|a| a.processed).sum();
            let failures = run
                .total_failures
                .map_or_else(|| "unknown".to_string(), |n| n.to_string());
            println!(
                "\nRescan {}: {} processed, {} failure(s) across {} account(s).",
                status.as_str(),
                processed,
                failures,
                run.accounts.len()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_doc_carries_the_load_bearing_fields() {
        let run = RescanRun {
            accounts: vec![AccountReport {
                email: "a@x.io".to_string(),
                cursor_reset: true,
                processed: 3,
                skipped: 1,
                failures: 0,
            }],
            total_failures: Some(0),
            service_success: true,
        };
        let doc = RescanOutputDoc {
            host: "auberge",
            account: None,
            status: run.status().as_str(),
            accounts: &run.accounts,
            total_failures: run.total_failures,
        };
        let value = serde_json::to_value(&doc).unwrap();
        assert_eq!(value["status"], "clean");
        assert_eq!(value["accounts"][0]["processed"], 3);
        assert_eq!(value["accounts"][0]["cursor_reset"], true);
        assert_eq!(value["total_failures"], 0);
    }
}
