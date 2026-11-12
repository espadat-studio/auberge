use crate::services::bichon::api::StoreEnvelope;
use chrono::{DateTime, Datelike, NaiveDate};
use eyre::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    Covered,
    Gap,
}

impl CoverageStatus {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Covered => 0,
            Self::Gap => 1,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Covered => "covered",
            Self::Gap => "gap",
        }
    }
}

/// A store message the archive holds no identity evidence for, reported in
/// the canonical form the sidecar would have recorded (ADR-0013), so the
/// operator can grep the archive for it directly.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct MissingMessage {
    pub message_id: String,
    pub date: String,
    pub uid: u32,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct UnverifiableCounts {
    pub contested: Vec<ContestedId>,
}

/// A Message-ID claimed more than once on either side: by several store
/// messages, or by several archived bodies. The archive dedups only
/// byte-identical bodies, so a second body under one id is a distinct message.
/// Any sender can claim an id, so a match by id cannot show which body is
/// held for which store message. Counted instead: fewer archived bodies than
/// store messages is a gap.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct ContestedId {
    pub message_id: String,
    pub store: usize,
    pub archive: usize,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct CoverageReport {
    pub store_messages: usize,
    pub matched: usize,
    pub missing: Vec<MissingMessage>,
    pub unverifiable: UnverifiableCounts,
}

impl CoverageReport {
    pub fn status(&self) -> CoverageStatus {
        if self.missing.is_empty()
            && self
                .unverifiable
                .contested
                .iter()
                .all(|c| c.store <= c.archive)
        {
            CoverageStatus::Covered
        } else {
            CoverageStatus::Gap
        }
    }
}

/// One `.meta.json` sidecar as the Host reports it: its path (which encodes
/// the message Date as YYYY/MM), the folder observed at first sight, and the
/// canonical message identity — empty when the sidecar predates keying.
#[derive(Debug, PartialEq, Eq)]
pub struct SidecarRow {
    pub path: String,
    pub folder: String,
    pub message_id: String,
}

/// Emits "path<TAB>folder<TAB>message_id" for every sidecar of the account —
/// the same walk gate 3 of bichon-expunge.sh batches, minus its window
/// filter, which happens locally where the cutoff is already parsed. Exit 3
/// distinguishes "no such archive directory" from a broken walk.
pub fn sidecar_rows_command(archive_dir: &str) -> String {
    format!(
        r#"sudo sh -c '[ -d {archive_dir} ] || exit 3; find {archive_dir} -regextype posix-extended -regex ".*/[0-9]{{4}}/[0-9]{{2}}/[^/]+\.meta\.json" -print0 | xargs -0 -r jq -r "[input_filename, .folder, (.message_id // \"\")] | @tsv"'"#
    )
}

// The archive path reaches a single-quoted remote sh -c line; rejecting
// anything beyond safe path characters is simpler and stricter than escaping.
pub fn validate_archive_path_for_shell(path: &str) -> Result<()> {
    let ok = path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'));
    if !ok {
        eyre::bail!("archive path '{path}' contains characters unsafe for a remote shell");
    }
    Ok(())
}

pub fn parse_sidecar_rows(stdout: &str) -> Result<Vec<SidecarRow>> {
    let mut rows = Vec::new();
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let (Some(path), Some(folder), Some(message_id)) =
            (parts.next(), parts.next(), parts.next())
        else {
            eyre::bail!("sidecar row the Host emitted cannot be read: {line}");
        };
        rows.push(SidecarRow {
            path: path.to_string(),
            folder: folder.to_string(),
            message_id: message_id.to_string(),
        });
    }
    Ok(rows)
}

enum StoreId {
    Synthetic,
    Canonical(String),
}

// Bichon's envelope field is the header value with angle brackets stripped,
// or — for a message with no Message-ID header — a synthetic
// `<hex.timestamp.pid@bichon>` with brackets kept (measured in ADR-0013).
// The bracket strip is defensive symmetry with the sidecar's canonical form;
// a real value has already lost them upstream.
fn canonicalize_store_id(raw: &str) -> StoreId {
    let trimmed = raw.trim();
    if trimmed.starts_with('<') && trimmed.ends_with("@bichon>") {
        return StoreId::Synthetic;
    }
    let stripped = trimmed
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(trimmed);
    StoreId::Canonical(stripped.to_string())
}

// The find regex on the Host guarantees this shape; a path that breaks it
// means the walk changed underneath this parser and must not count.
fn path_year_month(path: &str) -> Option<(i32, u32)> {
    let mut segments = path.rsplit('/');
    segments.next()?;
    let month: u32 = segments.next()?.parse().ok()?;
    let year: i32 = segments.next()?.parse().ok()?;
    Some((year, month))
}

/// Compares one folder's in-window store messages against the account's
/// archived identities (ADR-0088). `envelopes` is the whole account's store,
/// unwindowed; `rows` is the account-wide sidecar walk.
///
/// The window is "dated strictly before the cutoff day", the bound Bichon's
/// `before` filter would apply to the same `date` field. It scopes only which
/// store messages are judged. Their evidence is account-wide: a body under
/// any folder or month vouches, since the sidecar's folder is first-sight
/// restore metadata and a moved body is deduped, never re-archived. A
/// Message-ID's claimants are likewise counted across the whole store, so a
/// self-sent mail's other copy, in another folder or past the cutoff, cannot
/// borrow the one archived body.
///
/// A store message with no Message-ID carries a synthetic id that regenerates
/// on re-import; the archive keys its body by hash instead (ADR-0013).
/// `body_keys` maps each in-window one's envelope id to that hash, computed
/// from the body the store serves, and it matches like any other identity.
/// Byte-identical bodies are one identity, so they are never contested.
///
/// The unkeyed-sidecar refusal stays scoped to the folder through the cutoff
/// month: a sidecar with no `message_id` there is an unknown, not a zero, so
/// it fails by name.
pub fn compare_coverage(
    envelopes: &[StoreEnvelope],
    rows: &[SidecarRow],
    folder: &str,
    cutoff: NaiveDate,
    body_keys: &HashMap<String, String>,
) -> Result<CoverageReport> {
    let cutoff_ms = midnight_ms(cutoff);
    let cutoff_ym = (cutoff.year(), cutoff.month());

    let mut archived: HashMap<&str, usize> = HashMap::new();
    for row in rows {
        if !row.message_id.is_empty() {
            *archived.entry(row.message_id.as_str()).or_default() += 1;
        }
        if row.folder != folder {
            continue;
        }
        let Some(ym) = path_year_month(&row.path) else {
            eyre::bail!(
                "archived sidecar path without a YYYY/MM partition: {}",
                row.path
            );
        };
        if ym > cutoff_ym {
            continue;
        }
        if row.message_id.is_empty() {
            eyre::bail!("an archived sidecar carries no message_id: {}", row.path);
        }
    }

    let mut account_claims: HashMap<String, usize> = HashMap::new();
    let mut store_messages = 0usize;
    let mut matched = 0usize;
    let mut missing = Vec::new();
    let mut claims: Vec<(String, &StoreEnvelope)> = Vec::new();
    for envelope in envelopes {
        let StoreId::Canonical(id) = canonicalize_store_id(&envelope.message_id) else {
            if !in_folder_before(envelope, envelope.date, folder, cutoff_ms)? {
                continue;
            }
            store_messages += 1;
            let Some(key) = body_keys.get(&envelope.id) else {
                eyre::bail!(
                    "no body hash for the synthetic store message uid {} (envelope {})",
                    envelope.uid,
                    envelope.id
                );
            };
            if archived.contains_key(key.as_str()) {
                matched += 1;
            } else {
                missing.push(MissingMessage {
                    message_id: key.clone(),
                    date: iso_date(envelope.date),
                    uid: envelope.uid,
                });
            }
            continue;
        };
        *account_claims.entry(id.clone()).or_default() += 1;
        if in_folder_before(envelope, envelope.date, folder, cutoff_ms)? {
            store_messages += 1;
            claims.push((id, envelope));
        }
    }

    let mut contested = Vec::new();
    let mut seen_contested = HashSet::new();
    for (id, envelope) in claims {
        let store = account_claims[&id];
        let archive = archived.get(id.as_str()).copied().unwrap_or(0);
        if archive == 0 {
            missing.push(MissingMessage {
                message_id: id,
                date: iso_date(envelope.date),
                uid: envelope.uid,
            });
        } else if store == 1 && archive == 1 {
            matched += 1;
        } else if seen_contested.insert(id.clone()) {
            contested.push(ContestedId {
                message_id: id,
                store,
                archive,
            });
        }
    }

    Ok(CoverageReport {
        store_messages,
        matched,
        missing,
        unverifiable: UnverifiableCounts { contested },
    })
}

/// The in-window store messages of `folder` whose identity is their body:
/// the ones `compare_coverage` needs a `body_keys` entry for.
pub fn synthetic_in_window<'a>(
    envelopes: &'a [StoreEnvelope],
    folder: &str,
    cutoff: NaiveDate,
) -> Result<Vec<&'a StoreEnvelope>> {
    let cutoff_ms = midnight_ms(cutoff);
    let mut synthetic = Vec::new();
    for envelope in envelopes {
        if matches!(
            canonicalize_store_id(&envelope.message_id),
            StoreId::Synthetic
        ) && in_folder_before(envelope, envelope.date, folder, cutoff_ms)?
        {
            synthetic.push(envelope);
        }
    }
    Ok(synthetic)
}

/// The archive's key for a body with no Message-ID: `canonical_message_id`
/// in bichon-archive.sh hashes the downloaded `.eml` the same way.
pub fn body_key(body: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, body);
    let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// The store side of the count gate (ADR-0088): the folder's raw message
/// count received strictly before the cutoff day, on the IMAP internal date
/// the `SEARCH BEFORE` it is compared to uses. Raw, not distinct: a folder
/// holding one Message-ID twice holds two messages on both sides.
///
/// Bichon records a missing internal date as 0. That falls inside every
/// window, as it does for Bichon's own `internal_date_before` filter, so the
/// message is counted and its uid listed in `without_internal_date`: the
/// server can place it, the store cannot, so the expunge script checks each
/// uid against its IMAP window before trusting the count.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct StoreCount {
    #[serde(rename = "store_internal_date_count")]
    pub before_cutoff: usize,
    #[serde(rename = "store_without_internal_date")]
    pub without_internal_date: Vec<u32>,
}

pub fn count_store_messages(
    envelopes: &[StoreEnvelope],
    folder: &str,
    cutoff: NaiveDate,
) -> Result<StoreCount> {
    let cutoff_ms = midnight_ms(cutoff);
    let mut count = StoreCount {
        before_cutoff: 0,
        without_internal_date: Vec::new(),
    };
    for envelope in envelopes {
        if !in_folder_before(envelope, envelope.internal_date, folder, cutoff_ms)? {
            continue;
        }
        count.before_cutoff += 1;
        if envelope.internal_date <= 0 {
            count.without_internal_date.push(envelope.uid);
        }
    }
    Ok(count)
}

fn midnight_ms(day: NaiveDate) -> i64 {
    day.and_hms_opt(0, 0, 0)
        .expect("midnight is a valid time")
        .and_utc()
        .timestamp_millis()
}

fn in_folder_before(
    envelope: &StoreEnvelope,
    at_ms: i64,
    folder: &str,
    cutoff_ms: i64,
) -> Result<bool> {
    if at_ms >= cutoff_ms {
        return Ok(false);
    }
    match envelope.mailbox_name.as_deref() {
        Some(name) => Ok(name == folder),
        None => eyre::bail!(
            "the store returned an envelope without a mailbox_name (uid {}); refusing to attribute it to a folder",
            envelope.uid
        ),
    }
}

fn iso_date(epoch_ms: i64) -> String {
    DateTime::from_timestamp_millis(epoch_ms)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| format!("epoch_ms:{epoch_ms}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(message_id: &str, mailbox: Option<&str>, date: i64, uid: u32) -> StoreEnvelope {
        StoreEnvelope {
            id: format!("env-{uid}"),
            message_id: message_id.to_string(),
            mailbox_name: mailbox.map(String::from),
            date,
            internal_date: date,
            uid,
        }
    }

    fn row(path: &str, folder: &str, message_id: &str) -> SidecarRow {
        SidecarRow {
            path: path.to_string(),
            folder: folder.to_string(),
            message_id: message_id.to_string(),
        }
    }

    const CUTOFF: NaiveDate = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
    const IN_WINDOW: i64 = 1_767_225_600_000;
    const AFTER_CUTOFF: i64 = 1_790_000_000_000;

    #[test]
    fn a_message_archived_under_another_folder_is_matched() {
        let report = compare_coverage(
            &[envelope("moved@x.io", Some("INBOX"), IN_WINDOW, 1)],
            &[row("/a/2024/01/1.meta.json", "Archive.2024", "moved@x.io")],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.matched, 1);
        assert!(report.missing.is_empty());
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn a_cross_folder_claimant_makes_one_body_a_contested_gap() {
        let report = compare_coverage(
            &[
                envelope("self@x.io", Some("INBOX"), IN_WINDOW, 1),
                envelope("self@x.io", Some("Sent"), IN_WINDOW, 2),
            ],
            &[row("/a/2026/01/1.meta.json", "Sent", "self@x.io")],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.store_messages, 1);
        assert_eq!(report.matched, 0);
        assert_eq!(
            report.unverifiable.contested,
            vec![ContestedId {
                message_id: "self@x.io".to_string(),
                store: 2,
                archive: 1,
            }]
        );
        assert_eq!(report.status(), CoverageStatus::Gap);
    }

    #[test]
    fn a_claimant_outside_the_window_still_counts() {
        let report = compare_coverage(
            &[
                envelope("x@x.io", Some("INBOX"), IN_WINDOW, 1),
                envelope("x@x.io", Some("INBOX"), AFTER_CUTOFF, 2),
            ],
            &[row("/a/2026/01/1.meta.json", "INBOX", "x@x.io")],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.store_messages, 1);
        assert_eq!(report.unverifiable.contested[0].store, 2);
        assert_eq!(report.status(), CoverageStatus::Gap);
    }

    #[test]
    fn a_message_dated_at_the_cutoff_midnight_is_outside_the_window() {
        let midnight = CUTOFF
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        let report = compare_coverage(
            &[
                envelope("in@x.io", Some("INBOX"), midnight - 1, 1),
                envelope("out@x.io", Some("INBOX"), midnight, 2),
            ],
            &[],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.store_messages, 1);
        assert_eq!(report.missing[0].message_id, "in@x.io");
    }

    #[test]
    fn every_store_message_archived_is_covered() {
        let report = compare_coverage(
            &[
                envelope("a@x.io", Some("INBOX"), 1_600_000_000_000, 1),
                envelope("b@x.io", Some("INBOX"), 1_600_000_000_000, 2),
            ],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "a@x.io"),
                row("/a/2026/02/2.meta.json", "INBOX", "b@x.io"),
            ],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.status(), CoverageStatus::Covered);
        assert_eq!(report.status().exit_code(), 0);
        assert_eq!(report.store_messages, 2);
        assert_eq!(report.matched, 2);
        assert!(report.missing.is_empty());
    }

    // The defect this closes (#400): a message the archive never ingested,
    // numerically masked by surplus sidecars of other messages, is named.
    #[test]
    fn an_unarchived_store_message_is_a_named_gap() {
        let report = compare_coverage(
            &[envelope("ghost@x.io", Some("INBOX"), 1_767_225_600_000, 42)],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "a@x.io"),
                row("/a/2026/01/2.meta.json", "INBOX", "b@x.io"),
            ],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.status(), CoverageStatus::Gap);
        assert_eq!(report.status().exit_code(), 1);
        assert_eq!(
            report.missing,
            vec![MissingMessage {
                message_id: "ghost@x.io".to_string(),
                date: "2026-01-01".to_string(),
                uid: 42,
            }]
        );
    }

    // Defensive symmetry: should Bichon ever keep the brackets of a real
    // header value, it still matches the sidecar's stripped canonical form.
    #[test]
    fn a_bracket_kept_store_id_matches_the_stripped_sidecar_id() {
        let report = compare_coverage(
            &[envelope("<a@x.io>", Some("INBOX"), 0, 1)],
            &[row("/a/2026/01/1.meta.json", "INBOX", "a@x.io")],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(report.matched, 1);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    fn keys(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(id, key)| (id.to_string(), key.to_string()))
            .collect()
    }

    fn synthetic(id: &str, mailbox: &str, date: i64, uid: u32) -> StoreEnvelope {
        StoreEnvelope {
            id: id.to_string(),
            ..envelope("<0f2a.1778017746410.15@bichon>", Some(mailbox), date, uid)
        }
    }

    #[test]
    fn a_synthetic_message_whose_body_hash_is_archived_anywhere_is_matched() {
        let report = compare_coverage(
            &[synthetic("e739", "INBOX", IN_WINDOW, 739)],
            &[row("/a/1970/01/1.meta.json", "Sent", "sha256:aa")],
            "INBOX",
            CUTOFF,
            &keys(&[("e739", "sha256:aa")]),
        )
        .unwrap();

        assert_eq!(report.store_messages, 1);
        assert_eq!(report.matched, 1);
        assert!(report.missing.is_empty());
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    // #1316: four copies of one quota notice used to vouch for four
    // synthetic messages; now each must find its own body.
    #[test]
    fn duplicate_hash_sidecars_do_not_vouch_for_another_body() {
        let report = compare_coverage(
            &[synthetic("e1", "INBOX", IN_WINDOW, 1)],
            &[
                row("/a/1970/01/1.meta.json", "INBOX", "sha256:aa"),
                row("/a/1970/01/2.meta.json", "INBOX", "sha256:aa"),
            ],
            "INBOX",
            CUTOFF,
            &keys(&[("e1", "sha256:bb")]),
        )
        .unwrap();

        assert_eq!(report.matched, 0);
        assert_eq!(
            report.missing,
            vec![MissingMessage {
                message_id: "sha256:bb".to_string(),
                date: "2026-01-01".to_string(),
                uid: 1,
            }]
        );
        assert_eq!(report.status(), CoverageStatus::Gap);
    }

    #[test]
    fn a_synthetic_message_without_a_body_hash_fails_loudly() {
        let err = compare_coverage(
            &[synthetic("e1", "INBOX", IN_WINDOW, 1)],
            &[],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("uid 1"));
    }

    #[test]
    fn only_in_window_synthetic_messages_of_the_folder_need_a_body() {
        let envelopes = [
            synthetic("in", "INBOX", IN_WINDOW, 1),
            synthetic("late", "INBOX", AFTER_CUTOFF, 2),
            synthetic("sent", "Sent", IN_WINDOW, 3),
            envelope("a@x.io", Some("INBOX"), IN_WINDOW, 4),
        ];
        let ids: Vec<&str> = synthetic_in_window(&envelopes, "INBOX", CUTOFF)
            .unwrap()
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, vec!["in"]);
    }

    #[test]
    fn body_key_is_the_archive_sha256_form() {
        assert_eq!(
            body_key(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // INBOX must not swallow INBOX2 on either side — the same exactness the
    // bash gate asserts of sidecar_rows_for_folder.
    #[test]
    fn folder_scoping_is_exact_on_both_sides() {
        let report = compare_coverage(
            &[
                envelope("a@x.io", Some("INBOX"), 0, 1),
                envelope("other@x.io", Some("INBOX2"), 0, 2),
            ],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "a@x.io"),
                row("/a/2026/01/2.meta.json", "INBOX2", "other@x.io"),
            ],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.store_messages, 1);
        assert_eq!(report.matched, 1);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn an_unkeyed_sidecar_in_scope_fails_by_name() {
        let err = compare_coverage(
            &[],
            &[row("/a/2026/01/1.meta.json", "INBOX", "")],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("/a/2026/01/1.meta.json"));
    }

    // The bash gate only refuses unkeyed sidecars inside the probed scope;
    // this comparison must not be stricter, or a stale sidecar in an excluded
    // folder would block every expunge of the account.
    #[test]
    fn an_unkeyed_sidecar_outside_the_scope_is_ignored() {
        let report = compare_coverage(
            &[envelope("a@x.io", Some("INBOX"), 0, 1)],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "a@x.io"),
                row("/a/2026/01/2.meta.json", "Trash", ""),
                row("/a/2026/06/3.meta.json", "INBOX", ""),
            ],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn a_store_envelope_without_a_mailbox_name_fails_loudly() {
        let err = compare_coverage(
            &[envelope("a@x.io", None, 0, 7)],
            &[],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("uid 7"));
    }

    #[test]
    fn an_envelope_past_the_cutoff_without_a_mailbox_name_does_not_block() {
        let report = compare_coverage(
            &[
                envelope("a@x.io", Some("INBOX"), IN_WINDOW, 1),
                envelope("b@x.io", None, AFTER_CUTOFF, 2),
            ],
            &[row("/a/2026/01/1.meta.json", "INBOX", "a@x.io")],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn an_empty_window_is_covered() {
        let report = compare_coverage(&[], &[], "INBOX", CUTOFF, &HashMap::new()).unwrap();
        assert_eq!(report.store_messages, 0);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn parse_sidecar_rows_keeps_an_empty_trailing_id() {
        let rows = parse_sidecar_rows(
            "/a/2026/01/1.meta.json\tINBOX\ta@x.io\n/a/2026/01/2.meta.json\tINBOX\t\n",
        )
        .unwrap();
        assert_eq!(
            rows,
            vec![
                SidecarRow {
                    path: "/a/2026/01/1.meta.json".to_string(),
                    folder: "INBOX".to_string(),
                    message_id: "a@x.io".to_string(),
                },
                SidecarRow {
                    path: "/a/2026/01/2.meta.json".to_string(),
                    folder: "INBOX".to_string(),
                    message_id: String::new(),
                },
            ]
        );
    }

    #[test]
    fn parse_sidecar_rows_refuses_a_row_missing_fields() {
        let err = parse_sidecar_rows("/a/2026/01/1.meta.json\n").unwrap_err();
        assert!(format!("{err}").contains("/a/2026/01/1.meta.json"));
    }

    #[test]
    fn parse_sidecar_rows_of_nothing_is_empty() {
        assert!(parse_sidecar_rows("").unwrap().is_empty());
    }

    #[test]
    fn sidecar_rows_command_guards_and_batches() {
        let cmd = sidecar_rows_command("/var/lib/bichon-archive/a@x.io");
        assert!(cmd.contains("[ -d /var/lib/bichon-archive/a@x.io ] || exit 3"));
        assert!(cmd.contains("xargs -0 -r jq"));
    }

    #[test]
    fn archive_path_validation_rejects_shell_metacharacters() {
        assert!(validate_archive_path_for_shell("/var/lib/bichon-archive").is_ok());
        assert!(validate_archive_path_for_shell("/var/lib/x'; rm -rf /").is_err());
        assert!(validate_archive_path_for_shell("relative/path").is_err());
        assert!(validate_archive_path_for_shell("/with space").is_err());
    }

    #[test]
    fn two_store_messages_claiming_one_archived_id_are_a_contested_gap() {
        let report = compare_coverage(
            &[
                envelope("x@x.io", Some("INBOX"), 0, 1),
                envelope("x@x.io", Some("INBOX"), 0, 2),
            ],
            &[row("/a/2026/01/1.meta.json", "INBOX", "x@x.io")],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.matched, 0);
        assert!(report.missing.is_empty());
        assert_eq!(
            report.unverifiable.contested,
            vec![ContestedId {
                message_id: "x@x.io".to_string(),
                store: 2,
                archive: 1,
            }]
        );
        assert_eq!(report.status(), CoverageStatus::Gap);
    }

    #[test]
    fn an_id_archived_with_two_bodies_is_unverifiable_not_matched() {
        let report = compare_coverage(
            &[envelope("x@x.io", Some("INBOX"), 0, 1)],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "x@x.io"),
                row("/a/2026/01/2.meta.json", "INBOX", "x@x.io"),
            ],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.matched, 0);
        assert_eq!(report.unverifiable.contested.len(), 1);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn a_contested_id_with_a_body_per_store_message_is_covered() {
        let report = compare_coverage(
            &[
                envelope("x@x.io", Some("INBOX"), 0, 1),
                envelope("x@x.io", Some("INBOX"), 0, 2),
            ],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "x@x.io"),
                row("/a/2026/01/2.meta.json", "INBOX", "x@x.io"),
            ],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.store_messages, 2);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn store_messages_claiming_an_unarchived_id_stay_named_missing() {
        let report = compare_coverage(
            &[
                envelope("x@x.io", Some("INBOX"), 0, 1),
                envelope("x@x.io", Some("INBOX"), 0, 2),
            ],
            &[],
            "INBOX",
            CUTOFF,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(report.missing.len(), 2);
        assert!(report.unverifiable.contested.is_empty());
    }

    #[test]
    fn a_gap_from_missing_renders_the_gap_status() {
        let report = CoverageReport {
            store_messages: 3,
            matched: 2,
            missing: vec![MissingMessage {
                message_id: "m@x.io".to_string(),
                date: "2026-01-01".to_string(),
                uid: 1,
            }],
            unverifiable: UnverifiableCounts {
                contested: Vec::new(),
            },
        };
        assert_eq!(report.status().as_str(), "gap");
    }
}
