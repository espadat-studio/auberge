use crate::services::progress::Progress;
use crate::services::ssh::SshSession;
use eyre::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub const ARCHIVE_DIR: &str = "/var/lib/bichon-archive";
pub const STATE_DIR: &str = "/var/lib/bichon-archive/.state";
pub const ARCHIVE_SERVICE: &str = "bichon-archive.service";
pub const ARCHIVE_USER: &str = "bichon";
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescanStatus {
    Clean,
    RunFailures,
    OperationalError,
}

impl RescanStatus {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Clean => 0,
            Self::RunFailures => 1,
            Self::OperationalError => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::RunFailures => "run_failures",
            Self::OperationalError => "operational_error",
        }
    }
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct AccountReport {
    pub email: String,
    pub cursor_reset: bool,
    pub processed: u64,
    pub skipped: u64,
    pub failures: u64,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct RescanRun {
    pub accounts: Vec<AccountReport>,
    pub total_failures: Option<u64>,
    pub service_success: bool,
}

impl RescanRun {
    pub fn status(&self) -> RescanStatus {
        if self.total_failures.unwrap_or(0) > 0 {
            RescanStatus::RunFailures
        } else if self.service_success {
            RescanStatus::Clean
        } else {
            RescanStatus::OperationalError
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RescanOutcome {
    Refused { stale_sidecars: Vec<String> },
    Busy,
    Ran(RescanRun),
}

pub fn sanitize_email(email: &str) -> String {
    email.replace('/', "_")
}

// Cursor paths and reset commands interpolate the email into a remote shell
// line; rejecting anything beyond the address characters Bichon accepts is
// simpler and stricter than escaping. verify-coverage interpolates the same
// value into its sidecar walk, so it applies the same rule.
pub(crate) fn validate_email_for_shell(email: &str) -> Result<()> {
    let ok = !email.is_empty()
        && email
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-' | '+' | '/'));
    if !ok {
        eyre::bail!("account email '{email}' contains characters unsafe for a remote shell");
    }
    Ok(())
}

pub fn parse_stale_sidecars(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

pub fn is_service_active(stdout: &str) -> bool {
    matches!(
        stdout.trim(),
        "active" | "activating" | "reloading" | "deactivating"
    )
}

pub fn cursor_reset_command(email: &str) -> String {
    let cursor = format!("{STATE_DIR}/{}.cursor", sanitize_email(email));
    format!(
        r#"sudo -u {ARCHIVE_USER} sh -c 'umask 077; mkdir -p {STATE_DIR} && printf "0\n" > {cursor}'"#
    )
}

pub fn parse_journal_cursor(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("-- cursor: "))
        .map(|cursor| cursor.trim().to_string())
        .filter(|cursor| !cursor.is_empty())
}

fn validate_cursor_for_shell(cursor: &str) -> Result<()> {
    let ok = cursor.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '=' | ';' | '-' | '_' | '.' | ':' | '+' | '/')
    });
    if !ok {
        eyre::bail!("journal cursor '{cursor}' contains characters unsafe for a remote shell");
    }
    Ok(())
}

fn token_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|tok| {
        tok.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
    })
}

fn token_u64(line: &str, key: &str) -> Option<u64> {
    token_value(line, key)?.parse().ok()
}

pub fn parse_run_report(
    journal: &str,
    known_emails: &[String],
    reset_emails: &HashSet<String>,
) -> (Vec<AccountReport>, Option<u64>) {
    let safe_to_email = emails_by_safe_name(known_emails);
    let reset_safe: HashSet<String> = reset_emails.iter().map(|e| sanitize_email(e)).collect();

    let mut accounts = Vec::new();
    let mut total_failures = None;

    for line in journal.lines() {
        if let Some(total) = token_u64(line, "total_failures") {
            total_failures = Some(total);
            continue;
        }
        // The per-account summary is the only line carrying all three
        // counters; backfill and body_repair lines carry failures= too.
        let (Some(safe), Some(processed), Some(skipped), Some(failures)) = (
            token_value(line, "account"),
            token_u64(line, "processed"),
            token_u64(line, "skipped"),
            token_u64(line, "failures"),
        ) else {
            continue;
        };
        let email = email_for(safe, &safe_to_email);
        accounts.push(AccountReport {
            cursor_reset: reset_safe.contains(safe),
            email,
            processed,
            skipped,
            failures,
        });
    }

    accounts.sort_by(|a, b| a.email.cmp(&b.email));
    (accounts, total_failures)
}

fn emails_by_safe_name(known_emails: &[String]) -> HashMap<String, &String> {
    known_emails
        .iter()
        .map(|email| (sanitize_email(email), email))
        .collect()
}

fn email_for(safe: &str, safe_to_email: &HashMap<String, &String>) -> String {
    safe_to_email
        .get(safe)
        .map_or_else(|| safe.to_string(), |e| (*e).clone())
}

#[derive(Debug, PartialEq, Eq)]
struct UnitState {
    running: bool,
    succeeded: bool,
}

/// A start queued behind bichon.service leaves the unit inactive with a
/// pending job, so the job counts as running too.
fn parse_unit_state(stdout: &str) -> UnitState {
    let prop = |key: &str| {
        stdout
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map_or("", str::trim)
    };
    UnitState {
        running: !prop("Job").is_empty() || is_service_active(prop("ActiveState")),
        succeeded: prop("Result") == "success" && prop("ActiveState") != "failed",
    }
}

/// Where the pass is, read off the archive script's journal lines: the
/// account start line, one line per search page, and the account summary.
/// A host whose script predates the page line yields account progress only.
struct PassTracker<'a> {
    safe_to_email: HashMap<String, &'a String>,
    total: usize,
    finished: usize,
    current: Option<String>,
    page: Option<(u64, u64)>,
}

impl<'a> PassTracker<'a> {
    fn new(known_emails: &'a [String]) -> Self {
        Self {
            safe_to_email: emails_by_safe_name(known_emails),
            total: known_emails.len(),
            finished: 0,
            current: None,
            page: None,
        }
    }

    fn render(&self) -> String {
        let mut msg = format!(
            "{}/{} accounts",
            self.finished,
            self.total.max(self.finished)
        );
        if let Some(email) = &self.current {
            msg.push_str(&format!(", {email}"));
        }
        if let Some((page, total_pages)) = self.page {
            msg.push_str(&format!(" page {page}/{total_pages}"));
        }
        msg
    }

    fn observe(&mut self, line: &str, progress: &mut dyn Progress) {
        let Some(safe) = token_value(line, "account") else {
            return;
        };
        let email = email_for(safe, &self.safe_to_email);
        if let (Some(page), Some(total_pages)) =
            (token_u64(line, "page"), token_u64(line, "total_pages"))
        {
            self.current = Some(email);
            self.page = Some((page, total_pages));
        } else if token_u64(line, "cursor_ms").is_some() {
            self.current = Some(email);
            self.page = None;
        } else if let (Some(processed), Some(skipped), Some(failures)) = (
            token_u64(line, "processed"),
            token_u64(line, "skipped"),
            token_u64(line, "failures"),
        ) {
            self.finished += 1;
            self.current = None;
            self.page = None;
            let deduped =
                token_u64(line, "deduped").map_or_else(String::new, |d| format!(" deduped={d}"));
            let summary = format!("{email}: processed={processed} skipped={skipped}{deduped}");
            if failures > 0 {
                progress.error(&format!("{summary} failures={failures}"));
            } else {
                progress.success(&summary);
            }
        } else {
            return;
        }
        progress.task_started(&self.render());
    }
}

fn read_unit_state(ssh: &dyn SshSession) -> Result<UnitState> {
    let show = ssh.run(&format!(
        "sudo systemctl show {ARCHIVE_SERVICE} -p ActiveState -p Job -p Result"
    ))?;
    if !show.success {
        eyre::bail!(
            "could not read the state of {ARCHIVE_SERVICE}: {}",
            show.stderr_str().trim()
        );
    }
    Ok(parse_unit_state(&show.stdout_str()))
}

/// Reads the journal past `cursor`, feeds each new line to the tracker and
/// appends it to `journal`, and answers the cursor the next read starts after.
fn drain_journal(
    ssh: &dyn SshSession,
    cursor: String,
    journal: &mut String,
    tracker: &mut PassTracker,
    progress: &mut dyn Progress,
) -> Result<String> {
    let read = ssh.run(&format!(
        "sudo journalctl -u {ARCHIVE_SERVICE} --after-cursor '{cursor}' -o cat --no-pager --quiet --show-cursor"
    ))?;
    if !read.success {
        eyre::bail!(
            "could not read the archive run journal: {}",
            read.stderr_str().trim()
        );
    }
    let stdout = read.stdout_str();
    for line in stdout.lines().filter(|l| !l.starts_with("-- cursor: ")) {
        tracker.observe(line, progress);
        journal.push_str(line);
        journal.push('\n');
    }
    let Some(next) = parse_journal_cursor(&stdout) else {
        return Ok(cursor);
    };
    validate_cursor_for_shell(&next)?;
    Ok(next)
}

pub fn execute_rescan(
    ssh: &dyn SshSession,
    progress: &mut dyn Progress,
    emails_to_reset: &[String],
    known_emails: &[String],
    poll: Duration,
) -> Result<RescanOutcome> {
    for email in emails_to_reset {
        validate_email_for_shell(email)?;
    }

    let stale_check = ssh.run(&format!(
        r#"sudo sh -c '[ -d {ARCHIVE_DIR} ] || exit 0; grep -rL --include="*.meta.json" -e "\"message_id\"" {ARCHIVE_DIR} || true'"#
    ))?;
    if !stale_check.success {
        eyre::bail!(
            "sidecar precondition check failed on the host: {}",
            stale_check.stderr_str().trim()
        );
    }
    let stale = parse_stale_sidecars(&stale_check.stdout_str());
    if !stale.is_empty() {
        return Ok(RescanOutcome::Refused {
            stale_sidecars: stale,
        });
    }

    // `is-active` exits non-zero for inactive units, so only stdout answers
    // the question; transport failures surface on the next command instead.
    let active = ssh.run(&format!("sudo systemctl is-active {ARCHIVE_SERVICE}"))?;
    if is_service_active(&active.stdout_str()) {
        return Ok(RescanOutcome::Busy);
    }

    for email in emails_to_reset {
        let reset = ssh.run(&cursor_reset_command(email))?;
        if !reset.success {
            eyre::bail!(
                "cursor reset failed for {email}: {}",
                reset.stderr_str().trim()
            );
        }
    }

    // The run's report is read from the journal after a cursor captured
    // here, not by InvocationID: systemd drops that property once it
    // unloads an inactive oneshot, so it can be gone by the time the pass
    // finishes. Same pattern as the uidvalidity watch.
    let cursor_capture = ssh.run("sudo journalctl -n0 --show-cursor --quiet --no-pager")?;
    let Some(cursor) = parse_journal_cursor(&cursor_capture.stdout_str()) else {
        eyre::bail!(
            "could not capture a journal cursor: {}",
            cursor_capture.stderr_str().trim()
        );
    };
    validate_cursor_for_shell(&cursor)?;

    // Type=oneshot: a blocking start would hide the pass until it ends, so
    // it is queued and the journal is polled instead. A non-zero exit of the
    // pass is a verdict to report, read from the unit's Result once it stops.
    let start = ssh.run(&format!(
        "sudo systemctl start --no-block {ARCHIVE_SERVICE}"
    ))?;
    if !start.success {
        eyre::bail!(
            "could not start {ARCHIVE_SERVICE}: {}",
            start.stderr_str().trim()
        );
    }

    let mut tracker = PassTracker::new(known_emails);
    progress.task_started(&tracker.render());
    let mut journal = String::new();
    let mut cursor = cursor;
    // State before journal: once the unit reads as stopped, the read that
    // follows still sees every line the pass logged.
    let state = loop {
        let state = read_unit_state(ssh)?;
        cursor = drain_journal(ssh, cursor, &mut journal, &mut tracker, progress)?;
        if !state.running {
            break state;
        }
        std::thread::sleep(poll);
    };

    let reset_set: HashSet<String> = emails_to_reset.iter().cloned().collect();
    let (accounts, total_failures) = parse_run_report(&journal, known_emails, &reset_set);

    Ok(RescanOutcome::Ran(RescanRun {
        accounts,
        total_failures,
        // The script logs total_failures before every exit 0, so a success
        // without it is a start systemd dropped, not a pass that ran.
        service_success: state.succeeded && total_failures.is_some(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::progress::{MockProgress, ProgressEvent};
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};

    fn ok_with_stdout(stdout: &str) -> CommandResult {
        CommandResult {
            success: true,
            exit_code: Some(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn failed_with(stdout: &str, stderr: &str, code: i32) -> CommandResult {
        CommandResult {
            success: false,
            exit_code: Some(code),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    const CURSOR_STDOUT: &str = "-- cursor: s=0123456789ab;i=1f2;b=00aa;m=bb;t=cc;x=dd\n";

    fn emails(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn refuses_when_any_sidecar_lacks_message_id() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(
            "/var/lib/bichon-archive/a/2024/01/1.meta.json\n/var/lib/bichon-archive/a/2024/02/2.meta.json\n",
        ));

        let outcome = rescan(&mock, &["a@x.io"], &["a@x.io"]).0.unwrap();

        assert_eq!(
            outcome,
            RescanOutcome::Refused {
                stale_sidecars: vec![
                    "/var/lib/bichon-archive/a/2024/01/1.meta.json".to_string(),
                    "/var/lib/bichon-archive/a/2024/02/2.meta.json".to_string(),
                ]
            }
        );
        assert_eq!(
            mock.calls()[0],
            SshOp::Run(
                r#"sudo sh -c '[ -d /var/lib/bichon-archive ] || exit 0; grep -rL --include="*.meta.json" -e "\"message_id\"" /var/lib/bichon-archive || true'"#
                    .to_string()
            ),
            "the precondition walk reports sidecars missing message_id, not the ones carrying it"
        );
        // Refusal happens before any cursor is touched or the service started.
        assert_eq!(mock.calls().len(), 1);
    }

    #[test]
    fn busy_when_an_archive_run_is_active() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(ok_with_stdout("active\n"));

        let outcome = rescan(&mock, &["a@x.io"], &["a@x.io"]).0.unwrap();

        assert_eq!(outcome, RescanOutcome::Busy);
        assert_eq!(
            mock.calls()[1],
            SshOp::Run("sudo systemctl is-active bichon-archive.service".to_string())
        );
        assert_eq!(mock.calls().len(), 2);
    }

    const RUNNING: &str = "ActiveState=activating\nJob=\nResult=success\n";
    const QUEUED: &str = "ActiveState=inactive\nJob=4711\nResult=success\n";
    const FINISHED: &str = "ActiveState=inactive\nJob=\nResult=success\n";
    const FAILED: &str = "ActiveState=failed\nJob=\nResult=exit-code\n";

    // Everything a pass that is allowed to run issues before the first poll:
    // the precondition walk, the busy check, one reset per account, the
    // journal cursor and the non-blocking start.
    fn stage_through_start(mock: &MockSshSession, resets: usize) {
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        for _ in 0..resets {
            mock.stage_run_result(CommandResult::ok());
        }
        mock.stage_run_result(ok_with_stdout(CURSOR_STDOUT));
        mock.stage_run_result(CommandResult::ok());
    }

    fn stage_poll(mock: &MockSshSession, state: &str, journal: &str) {
        mock.stage_run_result(ok_with_stdout(state));
        mock.stage_run_result(ok_with_stdout(journal));
    }

    fn rescan(
        mock: &MockSshSession,
        reset: &[&str],
        known: &[&str],
    ) -> (Result<RescanOutcome>, Vec<ProgressEvent>) {
        let recorder = MockProgress::new();
        let mut progress = recorder.share();
        let outcome = execute_rescan(
            mock,
            &mut progress,
            &emails(reset),
            &emails(known),
            Duration::ZERO,
        );
        (outcome, recorder.events())
    }

    fn started(msg: &str) -> ProgressEvent {
        ProgressEvent::TaskStarted(msg.to_string())
    }

    #[test]
    fn clean_run_resets_selected_cursor_and_reports_all_accounts() {
        let mock = MockSshSession::new();
        stage_through_start(&mock, 1);
        stage_poll(
            &mock,
            RUNNING,
            "2026-07-31T09:00:00Z account=dev@x.io id=1 cursor_ms=0 since_ms=0\n\
             2026-07-31T09:00:01Z account=dev@x.io backfill repaired=0 failures=0\n\
             2026-07-31T09:00:02Z account=dev@x.io page=1 total_pages=2\n\
             -- cursor: s=aa;i=2\n",
        );
        stage_poll(
            &mock,
            FINISHED,
            "2026-07-31T09:03:00Z account=dev@x.io page=2 total_pages=2\n\
             2026-07-31T09:05:00Z account=dev@x.io processed=432 skipped=4527 deduped=3 failures=0\n\
             2026-07-31T09:05:01Z account=dev@x.io tag_snapshot entries=4\n\
             2026-07-31T09:06:00Z account=ops@x.io id=2 cursor_ms=5 since_ms=0\n\
             2026-07-31T09:06:00Z account=ops@x.io page=1 total_pages=1\n\
             2026-07-31T09:06:00Z account=ops@x.io processed=0 skipped=12 deduped=0 failures=0\n\
             2026-07-31T09:06:01Z total_failures=0\n\
             -- cursor: s=aa;i=9\n",
        );

        let (outcome, events) = rescan(&mock, &["dev@x.io"], &["dev@x.io", "ops@x.io"]);

        let RescanOutcome::Ran(run) = outcome.unwrap() else {
            panic!("expected Ran");
        };
        assert_eq!(run.status(), RescanStatus::Clean);
        assert_eq!(run.status().exit_code(), 0);
        assert_eq!(run.total_failures, Some(0));
        assert_eq!(
            run.accounts,
            vec![
                AccountReport {
                    email: "dev@x.io".to_string(),
                    cursor_reset: true,
                    processed: 432,
                    skipped: 4527,
                    failures: 0,
                },
                AccountReport {
                    email: "ops@x.io".to_string(),
                    cursor_reset: false,
                    processed: 0,
                    skipped: 12,
                    failures: 0,
                },
            ]
        );

        assert_eq!(
            events,
            [
                started("0/2 accounts"),
                started("0/2 accounts, dev@x.io"),
                started("0/2 accounts, dev@x.io page 1/2"),
                started("0/2 accounts, dev@x.io page 2/2"),
                ProgressEvent::Success(
                    "dev@x.io: processed=432 skipped=4527 deduped=3".to_string()
                ),
                started("1/2 accounts"),
                started("1/2 accounts, ops@x.io"),
                started("1/2 accounts, ops@x.io page 1/1"),
                ProgressEvent::Success("ops@x.io: processed=0 skipped=12 deduped=0".to_string()),
                started("2/2 accounts"),
            ]
        );

        let calls = mock.calls();
        assert_eq!(
            calls[2],
            SshOp::Run(
                r#"sudo -u bichon sh -c 'umask 077; mkdir -p /var/lib/bichon-archive/.state && printf "0\n" > /var/lib/bichon-archive/.state/dev@x.io.cursor'"#
                    .to_string()
            ),
            "only the selected account's cursor is reset"
        );
        assert_eq!(
            calls[3],
            SshOp::Run("sudo journalctl -n0 --show-cursor --quiet --no-pager".to_string())
        );
        assert_eq!(
            calls[4],
            SshOp::Run("sudo systemctl start --no-block bichon-archive.service".to_string()),
            "the pass is started without waiting, so the journal can be polled while it runs"
        );
        assert_eq!(
            calls[5],
            SshOp::Run(
                "sudo systemctl show bichon-archive.service -p ActiveState -p Job -p Result"
                    .to_string()
            )
        );
        assert_eq!(
            calls[6],
            SshOp::Run(
                "sudo journalctl -u bichon-archive.service --after-cursor 's=0123456789ab;i=1f2;b=00aa;m=bb;t=cc;x=dd' -o cat --no-pager --quiet --show-cursor"
                    .to_string()
            )
        );
        assert_eq!(
            calls[8],
            SshOp::Run(
                "sudo journalctl -u bichon-archive.service --after-cursor 's=aa;i=2' -o cat --no-pager --quiet --show-cursor"
                    .to_string()
            ),
            "each poll reads only what the previous one had not"
        );
        assert_eq!(
            calls.len(),
            9,
            "polling stops once the unit is no longer running"
        );
    }

    #[test]
    fn a_queued_start_is_still_waited_for() {
        let mock = MockSshSession::new();
        stage_through_start(&mock, 1);
        stage_poll(&mock, QUEUED, "");
        stage_poll(
            &mock,
            FINISHED,
            "account=a@x.io processed=1 skipped=0 deduped=0 failures=0\ntotal_failures=0\n",
        );

        let (outcome, _) = rescan(&mock, &["a@x.io"], &["a@x.io"]);

        let RescanOutcome::Ran(run) = outcome.unwrap() else {
            panic!("expected Ran");
        };
        assert_eq!(run.accounts.len(), 1);
        assert_eq!(mock.calls().len(), 9);
    }

    // A host still running an archive script that predates the page line
    // reports accounts only. The bar follows them without failing.
    #[test]
    fn a_host_without_page_lines_shows_account_progress_only() {
        let mock = MockSshSession::new();
        stage_through_start(&mock, 1);
        stage_poll(
            &mock,
            FINISHED,
            "account=a@x.io id=1 cursor_ms=0 since_ms=0\n\
             account=a@x.io processed=7 skipped=1 failures=0\n\
             total_failures=0\n",
        );

        let (outcome, events) = rescan(&mock, &["a@x.io"], &["a@x.io"]);

        assert!(matches!(outcome.unwrap(), RescanOutcome::Ran(_)));
        assert_eq!(
            events,
            [
                started("0/1 accounts"),
                started("0/1 accounts, a@x.io"),
                ProgressEvent::Success("a@x.io: processed=7 skipped=1".to_string()),
                started("1/1 accounts"),
            ]
        );
    }

    #[test]
    fn counted_failures_are_exit_one() {
        let mock = MockSshSession::new();
        stage_through_start(&mock, 1);
        stage_poll(
            &mock,
            FAILED,
            "account=dev@x.io processed=10 skipped=2 deduped=0 failures=3\ntotal_failures=3\n",
        );

        let (outcome, events) = rescan(&mock, &["dev@x.io"], &["dev@x.io"]);

        let RescanOutcome::Ran(run) = outcome.unwrap() else {
            panic!("expected Ran");
        };
        assert_eq!(run.status(), RescanStatus::RunFailures);
        assert_eq!(run.status().exit_code(), 1);
        assert!(
            events.contains(&ProgressEvent::Error(
                "dev@x.io: processed=10 skipped=2 deduped=0 failures=3".to_string()
            )),
            "an account that counted failures is not reported as a success: {events:?}"
        );
    }

    #[test]
    fn service_failure_without_a_report_is_operational() {
        let mock = MockSshSession::new();
        stage_through_start(&mock, 1);
        stage_poll(
            &mock,
            FAILED,
            "2026-07-31T09:00:00Z auth check failed: GET /api/v1/current-user did not return 2xx\n",
        );

        let (outcome, _) = rescan(&mock, &["dev@x.io"], &["dev@x.io"]);

        let RescanOutcome::Ran(run) = outcome.unwrap() else {
            panic!("expected Ran");
        };
        assert_eq!(run.total_failures, None);
        assert_eq!(run.status(), RescanStatus::OperationalError);
        assert_eq!(run.status().exit_code(), 2);
    }

    // A queued start that systemd drops because bichon.service failed leaves
    // the unit inactive with its previous Result, usually success. The script
    // never ran, so it never logged total_failures, and that is not clean.
    #[test]
    fn a_start_dropped_by_a_failed_dependency_is_operational() {
        let mock = MockSshSession::new();
        stage_through_start(&mock, 1);
        stage_poll(
            &mock,
            FINISHED,
            "Dependency failed for Bichon email archive.\n",
        );

        let (outcome, _) = rescan(&mock, &["a@x.io"], &["a@x.io"]);

        let RescanOutcome::Ran(run) = outcome.unwrap() else {
            panic!("expected Ran");
        };
        assert_eq!(run.status(), RescanStatus::OperationalError);
        assert_eq!(run.status().exit_code(), 2);
    }

    #[test]
    fn the_account_total_never_reads_below_the_accounts_finished() {
        let mock = MockSshSession::new();
        stage_through_start(&mock, 0);
        stage_poll(
            &mock,
            FINISHED,
            "account=a@x.io processed=1 skipped=0 deduped=0 failures=0\n\
             account=new@x.io processed=1 skipped=0 deduped=0 failures=0\n\
             total_failures=0\n",
        );

        let (_, events) = rescan(&mock, &[], &["a@x.io"]);

        assert_eq!(events.last(), Some(&started("2/2 accounts")));
    }

    #[test]
    fn a_start_that_cannot_be_queued_is_an_error() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        mock.stage_run_result(ok_with_stdout(CURSOR_STDOUT));
        mock.stage_run_result(failed_with("", "Unit bichon-archive.service not found.", 5));

        let (outcome, _) = rescan(&mock, &[], &["a@x.io"]);

        let err = outcome.unwrap_err();
        assert!(format!("{err}").contains("not found"), "{err}");
        assert_eq!(
            mock.calls().len(),
            4,
            "nothing is polled when no pass was started"
        );
    }

    #[test]
    fn an_unreadable_journal_mid_pass_is_an_error() {
        let mock = MockSshSession::new();
        stage_through_start(&mock, 1);
        mock.stage_run_result(ok_with_stdout(RUNNING));
        mock.stage_run_result(failed_with("", "journalctl: permission denied", 1));

        let (outcome, _) = rescan(&mock, &["a@x.io"], &["a@x.io"]);

        assert!(format!("{}", outcome.unwrap_err()).contains("archive run journal"));
    }

    #[test]
    fn parse_unit_state_reads_running_and_verdict() {
        assert_eq!(
            parse_unit_state(RUNNING),
            UnitState {
                running: true,
                succeeded: true
            }
        );
        assert_eq!(
            parse_unit_state(QUEUED),
            UnitState {
                running: true,
                succeeded: true
            }
        );
        assert_eq!(
            parse_unit_state(FINISHED),
            UnitState {
                running: false,
                succeeded: true
            }
        );
        assert_eq!(
            parse_unit_state(FAILED),
            UnitState {
                running: false,
                succeeded: false
            }
        );
    }

    #[test]
    fn rejects_email_with_shell_metacharacters() {
        let mock = MockSshSession::new();
        let err = rescan(
            &mock,
            &["evil'; rm -rf / #@x.io"],
            &["evil'; rm -rf / #@x.io"],
        )
        .0
        .unwrap_err();
        assert!(format!("{err}").contains("unsafe"));
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn parse_run_report_maps_sanitized_emails_and_flags_resets() {
        let journal = "account=weird_address@x.io processed=1 skipped=0 failures=0\n\
                       total_failures=0\n";
        let known = emails(&["weird/address@x.io"]);
        let reset: HashSet<String> = known.iter().cloned().collect();

        let (accounts, total) = parse_run_report(journal, &known, &reset);

        assert_eq!(total, Some(0));
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].email, "weird/address@x.io");
        assert!(accounts[0].cursor_reset);
    }

    #[test]
    fn parse_run_report_ignores_backfill_and_body_repair_lines() {
        let journal = "account=a@x.io backfill repaired=2 failures=1\n\
                       account=a@x.io body_repair repaired=1 failures=1\n\
                       account=a@x.io processed=5 skipped=0 failures=0\n\
                       total_failures=2\n";

        let (accounts, total) = parse_run_report(journal, &emails(&["a@x.io"]), &HashSet::new());

        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].processed, 5);
        assert_eq!(total, Some(2));
    }

    #[test]
    fn parse_run_report_keeps_unknown_accounts_under_their_safe_name() {
        let journal = "account=ghost@x.io processed=0 skipped=1 failures=0\n";

        let (accounts, total) = parse_run_report(journal, &[], &HashSet::new());

        assert_eq!(total, None);
        assert_eq!(accounts[0].email, "ghost@x.io");
        assert!(!accounts[0].cursor_reset);
    }

    #[test]
    fn cursor_reset_command_writes_zero_as_the_bichon_user() {
        let cmd = cursor_reset_command("dev/x@y.io");
        assert_eq!(
            cmd,
            r#"sudo -u bichon sh -c 'umask 077; mkdir -p /var/lib/bichon-archive/.state && printf "0\n" > /var/lib/bichon-archive/.state/dev_x@y.io.cursor'"#
        );
    }

    #[test]
    fn missing_journal_cursor_is_an_error() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        mock.stage_run_result(CommandResult::ok());
        mock.stage_run_result(ok_with_stdout(""));

        let err = rescan(&mock, &["a@x.io"], &["a@x.io"]).0.unwrap_err();
        assert!(format!("{err}").contains("journal cursor"));
        // The service is never started without a cursor to scope its report.
        assert_eq!(mock.calls().len(), 4);
    }

    #[test]
    fn unquotable_journal_cursor_is_an_error() {
        let mock = MockSshSession::new();
        mock.stage_run_result(ok_with_stdout(""));
        mock.stage_run_result(failed_with("inactive\n", "", 3));
        mock.stage_run_result(CommandResult::ok());
        mock.stage_run_result(ok_with_stdout("-- cursor: s=abc'; rm -rf / #\n"));

        let err = rescan(&mock, &["a@x.io"], &["a@x.io"]).0.unwrap_err();
        assert!(format!("{err}").contains("unsafe"));
        assert_eq!(mock.calls().len(), 4);
    }

    #[test]
    fn parse_journal_cursor_reads_the_cursor_line() {
        assert_eq!(
            parse_journal_cursor(CURSOR_STDOUT),
            Some("s=0123456789ab;i=1f2;b=00aa;m=bb;t=cc;x=dd".to_string())
        );
        assert_eq!(parse_journal_cursor(""), None);
        assert_eq!(parse_journal_cursor("-- cursor: \n"), None);
    }

    #[test]
    fn unreachable_host_fails_on_the_first_command() {
        let mock = MockSshSession::new();
        mock.stage_run_result(failed_with("", "ssh: connect to host: timed out", 255));

        let err = rescan(&mock, &["a@x.io"], &["a@x.io"]).0.unwrap_err();
        assert!(format!("{err}").contains("precondition"));
        assert_eq!(mock.calls().len(), 1);
    }
}
