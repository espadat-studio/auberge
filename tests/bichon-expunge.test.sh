#!/usr/bin/env bash
#
# tests/bichon-expunge.test.sh
#
# Unit tests for the pure, network-free logic in examples/bichon-expunge.sh —
# the last checks between an operator and permanently deleting mail from an
# Upstream Mailbox. Covers gate 3's coverage verdict (the store count bound
# and the refusal to guess), the case-insensitive folder-name near-miss, the Synced
# Folder eligibility verdict, and the --folder default/passthrough, all
# against in-memory input: no Host, no archive, no himalaya, no IMAP, no
# auberge.
#
# list_synced_folders is untested for the same reason — it shells out to
# `auberge bichon reconcile-folders` — but check_folder_synced, which consumes
# its output, needs no network and is covered below.
#
# Run: ./tests/bichon-expunge.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"
# shellcheck source=../examples/bichon-expunge.sh disable=SC1091
source "${REPO_ROOT}/examples/bichon-expunge.sh"

printf '\n== parse_identity_verdict\n'

# Feeds a verify-coverage JSON verdict to parse_identity_verdict on stdin —
# same shape as verdict_for above, so assert_fails has a command to run.
identity_verdict_of() {
  parse_identity_verdict <<<"$1"
}

COVERED_VERDICT='{"host":"h","account":"a@example.com","folder":"INBOX","before":"2026-05-01","status":"covered","store_messages":388,"store_internal_date_count":388,"store_without_internal_date":[],"matched":388,"missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'

parse_identity_verdict <<<"${COVERED_VERDICT}"
assert_eq 'a covered verdict is read as covered' 'covered' "${IDENTITY_STATUS}"
assert_eq 'a covered verdict extracts the store count' '388' "${IDENTITY_STORE}"
assert_eq 'a covered verdict extracts the internal-date store count' '388' "${STORE_COUNT}"
assert_eq 'a covered verdict has no undated store uids' '' "${STORE_UNDATED_UIDS}"

UNDATED_VERDICT='{"status":"covered","store_messages":3,"store_internal_date_count":3,"store_without_internal_date":[648,701],"missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'
parse_identity_verdict <<<"${UNDATED_VERDICT}"
assert_eq 'undated store uids are extracted' '648 701' "${STORE_UNDATED_UIDS}"

BAD_UNDATED_VERDICT='{"status":"covered","store_messages":3,"store_internal_date_count":3,"store_without_internal_date":["*"],"missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'
assert_fails 'a non-numeric undated uid is refused' identity_verdict_of "${BAD_UNDATED_VERDICT}"

# One of the two ids carries an internal space to prove the sample field is
# taken whole — a value split on spaces would truncate it at the first word.
GAP_VERDICT='{"status":"gap","store_messages":10,"store_internal_date_count":10,"store_without_internal_date":[],"matched":8,"missing":[{"message_id":"<abc@example.com>","date":"2026-01-01","uid":1},{"message_id":"<odd id@example.com>","date":"2026-01-02","uid":2}],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'

parse_identity_verdict <<<"${GAP_VERDICT}"
assert_eq 'a gap verdict extracts the missing count' '2' "${IDENTITY_MISSING}"
assert_eq 'the sample joins the missing ids, whole' \
  '<abc@example.com> <odd id@example.com>' "${IDENTITY_SAMPLE}"

MANY_MISSING_VERDICT='{"status":"gap","store_messages":10,"store_internal_date_count":10,"store_without_internal_date":[],"matched":6,"missing":[{"message_id":"id1"},{"message_id":"id2"},{"message_id":"id3"},{"message_id":"id4"}],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'

parse_identity_verdict <<<"${MANY_MISSING_VERDICT}"
assert_eq 'more than three missing ids still count all of them' '4' "${IDENTITY_MISSING}"
assert_eq 'but the sample holds only the first three' 'id1 id2 id3' "${IDENTITY_SAMPLE}"

UNVERIFIABLE_VERDICT='{"status":"gap","store_messages":7,"store_internal_date_count":7,"store_without_internal_date":[],"matched":2,"missing":[],"unverifiable":{"store_synthetic":5,"archive_sha256":2}}'

parse_identity_verdict <<<"${UNVERIFIABLE_VERDICT}"
assert_eq 'header-less store messages are extracted' '5' "${IDENTITY_SYNTHETIC}"
assert_eq 'sha256-keyed sidecars are extracted' '2' "${IDENTITY_SHA256}"

# Only a contested id with more store claimants than archived bodies is short;
# one the archive holds enough bodies for is counted, not a gap.
CONTESTED_VERDICT='{"status":"gap","store_messages":3,"store_internal_date_count":3,"store_without_internal_date":[],"matched":0,"missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0,"contested":[{"message_id":"x@example.com","store":2,"archive":1},{"message_id":"y@example.com","store":1,"archive":2}]}}'

parse_identity_verdict <<<"${CONTESTED_VERDICT}"
assert_eq 'only short contested ids are counted' '1' "${IDENTITY_CONTESTED}"

parse_identity_verdict <<<"${UNVERIFIABLE_VERDICT}"
assert_eq 'a verdict from before contested ids reads as none' '0' "${IDENTITY_CONTESTED}"

assert_fails 'non-JSON input is refused' identity_verdict_of 'not json at all'

NO_STATUS_VERDICT='{"store_messages":5,"missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'
assert_fails 'a verdict with no status is refused' identity_verdict_of "${NO_STATUS_VERDICT}"

BAD_STATUS_VERDICT='{"status":"unknown","store_messages":5,"missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'
assert_fails 'a status that is neither covered nor gap is refused' identity_verdict_of "${BAD_STATUS_VERDICT}"

# store_messages is a string here — not the shape auberge's contract promises
# — so length can no longer be trusted to compute it; a half-parsed verdict
# must not pass a gate on a guess.
BAD_COUNT_VERDICT='{"status":"covered","store_messages":"many","missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'
assert_fails 'a non-numeric count is refused' identity_verdict_of "${BAD_COUNT_VERDICT}"

# An auberge predating the store count would leave the count gate nothing to
# compare against; a missing bound must not read as zero or pass.
NO_STORE_COUNT_VERDICT='{"status":"covered","store_messages":5,"missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0}}'
assert_fails 'a verdict without the store count is refused' identity_verdict_of "${NO_STORE_COUNT_VERDICT}"

# A stale gap sample must not survive into a later covered verdict's globals.
parse_identity_verdict <<<"${GAP_VERDICT}"
parse_identity_verdict <<<"${COVERED_VERDICT}"
assert_eq 'a later covered verdict clears an earlier gap status' 'covered' "${IDENTITY_STATUS}"
assert_eq 'a later covered verdict clears an earlier missing count' '0' "${IDENTITY_MISSING}"
assert_eq 'a later covered verdict clears an earlier sample' '' "${IDENTITY_SAMPLE}"

printf '\n== folder_case_insensitive_match\n'

assert_eq 'a case mangled folder finds its advertised casing' 'Sent' \
  "$(folder_case_insensitive_match SENT <<<$'Archive\nINBOX\nSent\nTrash')"

assert_eq 'an exact name matches itself' 'INBOX' \
  "$(folder_case_insensitive_match INBOX <<<$'Archive\nINBOX\nSent\nTrash')"

assert_fails 'a substring is not a match' \
  folder_case_insensitive_match Sen <<<$'Archive\nINBOX\nSent\nTrash'

assert_fails 'no match returns non-zero' \
  folder_case_insensitive_match Trashed <<<$'Archive\nINBOX\nSent\nTrash'

assert_eq 'no match prints nothing' '' \
  "$(folder_case_insensitive_match Trashed <<<$'Archive\nINBOX\nSent\nTrash')"

printf '\n== resolve_folder\n'

# Forces the non-interactive branch: on a developer TTY, interactive() would
# otherwise open a real select menu and hang the suite.
# shellcheck disable=SC2034  # intentional: read by interactive() in the sourced script
no_input=true

folder=''
resolve_folder
assert_eq 'an omitted folder falls back to INBOX non-interactively' 'INBOX' "${folder}"

folder='Sent'
resolve_folder
assert_eq 'a passed folder is left untouched' 'Sent' "${folder}"

printf '\n== check_folder_synced\n'

# check_folder_synced calls die, which exits — run it in a subshell so that
# exit lands on the wrapper, not the suite. Functions are sourced, so a plain
# ( ... ) subshell still sees check_folder_synced.
synced_verdict() {
  (
    # shellcheck disable=SC2034  # intentional: read by check_folder_synced in the sourced script
    folder="$1" SYNCED_FOLDERS="$2" DRIFT_ADDED="$3" DRIFT_REMOVED="$4"
    # shellcheck disable=SC2034  # intentional: read by check_folder_synced in the sourced script
    account='a@example.com' host='h'
    check_folder_synced
  ) 2>&1
}

# Discards synced_verdict's output for exit-status-only assertions.
synced_ok() {
  synced_verdict "$1" "$2" "$3" "$4" >/dev/null
}

# True if $1 contains $2 — checks a die message names the right reason without
# pinning the whole remediation text.
contains() {
  [[ "$1" == *"$2"* ]]
}

SYNCED=$'INBOX\nSent'

assert_succeeds 'a synced folder is eligible' synced_ok Sent "${SYNCED}" '' ''
assert_fails 'a non-synced folder is refused' synced_ok Trash "${SYNCED}" '' ''

assert_succeeds 'the refusal names it as not a Synced Folder' \
  contains "$(synced_verdict Trash "${SYNCED}" '' '')" 'not a Synced Folder'

assert_succeeds 'a folder pending removal names the drift' \
  contains "$(synced_verdict Receipts "${SYNCED}" '' 'Receipts')" 'being removed'

assert_succeeds 'a folder pending addition names the drift' \
  contains "$(synced_verdict Newsletter "${SYNCED}" 'Newsletter' '')" 'not yet in the Synced Folder set'

# Sent is synced; the drift lists both name a different folder and must not
# override the eligibility this SYNCED_FOLDERS match already grants.
assert_succeeds 'eligibility beats drift lists for other folders' \
  synced_ok Sent "${SYNCED}" 'Newsletter' 'Receipts'

printf '\n== sweep_classify_accounts\n'

assert_eq 'accounts split into eligible, no-archive, and no-account' \
  $'eligible\ta@x.com\nno-archive\tb@x.com\nno-account\tstray@x.com' \
  "$(sweep_classify_accounts $'a@x.com\nb@x.com' $'a@x.com\nstray@x.com')"

# bichon writes the archive directory with '/' sanitized to '_'; the account
# name still has the slash, and the match must survive the difference.
assert_eq 'an account with a slash matches its sanitized directory' \
  $'eligible\ta/b@x.com' \
  "$(sweep_classify_accounts 'a/b@x.com' 'a_b@x.com')"

assert_eq 'no himalaya accounts leaves only orphan archive directories' \
  $'no-account\td@x.com' \
  "$(sweep_classify_accounts '' 'd@x.com')"

printf '\n== sweep_classify_accounts scoped by --account\n'

SWEEP_HIMALAYA=$'a@x.com\nb@x.com\nc@x.com'
SWEEP_ARCHIVE=$'a@x.com\nc@x.com\nstray@x.com'

assert_eq 'a scoped sweep classifies only the named accounts' \
  $'eligible\ta@x.com' \
  "$(sweep_classify_accounts "${SWEEP_HIMALAYA}" "${SWEEP_ARCHIVE}" 'a@x.com')"

assert_eq 'several named accounts all classify' \
  $'eligible\ta@x.com\neligible\tc@x.com' \
  "$(sweep_classify_accounts "${SWEEP_HIMALAYA}" "${SWEEP_ARCHIVE}" $'a@x.com\nc@x.com')"

# Naming an account the archive cannot vouch for must still produce its row:
# the sweep report is the only place the operator learns the account they asked
# for was skipped, and a dropped candidate leaves nothing to read.
assert_eq 'a named account with no archive is still classified' \
  $'no-archive\tb@x.com' \
  "$(sweep_classify_accounts "${SWEEP_HIMALAYA}" "${SWEEP_ARCHIVE}" 'b@x.com')"

# no-account means "no himalaya account matches this directory", so a scope
# expressed in himalaya account names can never contain one.
assert_eq 'a scoped sweep emits no orphan archive rows' \
  $'eligible\ta@x.com' \
  "$(sweep_classify_accounts 'a@x.com' "${SWEEP_ARCHIVE}" 'a@x.com')"

assert_eq 'an empty scope is the whole Host, not an empty sweep' \
  $'eligible\ta@x.com\nno-archive\tb@x.com\neligible\tc@x.com\nno-account\tstray@x.com' \
  "$(sweep_classify_accounts "${SWEEP_HIMALAYA}" "${SWEEP_ARCHIVE}" '')"

# --account is the mailbox email everywhere else in the script; the scope test
# must not quietly accept the sanitized directory name instead.
assert_eq 'scope matches the account name, not its sanitized directory' \
  $'eligible\ta/b@x.com' \
  "$(sweep_classify_accounts 'a/b@x.com' 'a_b@x.com' 'a/b@x.com')"

assert_eq 'the sanitized directory name is not a usable scope' '' \
  "$(sweep_classify_accounts 'a/b@x.com' 'a_b@x.com' 'a_b@x.com')"

printf '\n== sweep_classify_folders\n'

SWEEP_SYNCED=$'Archive\nINBOX\nSent'
SWEEP_UPSTREAM=$'Drafts\nINBOX\nSent\nTrash'

assert_eq 'folders split into eligible, missing-upstream, and drift' \
  $'eligible\tINBOX\neligible\tSent\nmissing-upstream\tArchive\ndrift-added\tNewsletter\ndrift-removed\tReceipts' \
  "$(sweep_classify_folders "${SWEEP_SYNCED}" "${SWEEP_UPSTREAM}" 'Newsletter' 'Receipts')"

assert_eq 'no drift adds no drift rows' \
  $'eligible\tINBOX\neligible\tSent\nmissing-upstream\tArchive' \
  "$(sweep_classify_folders "${SWEEP_SYNCED}" "${SWEEP_UPSTREAM}" '' '')"

printf '\n== sweep rows: filtering, totals, rendering\n'

SWEEP_ROWS=(
  $'ready\ta@x.com\tINBOX\t12\t'
  $'skip\ta@x.com\tSent\t0\tnothing in window'
  $'finding\tb@x.com\tINBOX\t0\t2 message(s) on the server not ingested by the store (IMAP 5, store 3)'
  $'ready\tb@x.com\tSent\t30\t'
)

assert_eq 'the grand total sums only the ready rows' '42' \
  "$(sweep_rows_with_status ready | sweep_sum_counts)"

assert_eq 'status filtering keeps whole rows' \
  $'ready\ta@x.com\tINBOX\t12\t\nready\tb@x.com\tSent\t30\t' \
  "$(sweep_rows_with_status ready)"

assert_eq 'a status no row holds filters to nothing' '' \
  "$(sweep_rows_with_status expunged)"

assert_eq 'an empty row set sums to zero' '0' \
  "$(printf '' | sweep_sum_counts)"

SWEEP_RENDERED=$(printf '%s\n' "${SWEEP_ROWS[@]}" | render_sweep_rows)

assert_succeeds 'a finding shouts in the report' \
  contains "${SWEEP_RENDERED}" 'FINDING'

assert_succeeds 'a finding carries its reason' \
  contains "${SWEEP_RENDERED}" 'not ingested by the store (IMAP 5, store 3)'

assert_succeeds 'a ready row carries its count' \
  contains "${SWEEP_RENDERED}" '12'

printf '\n== probe_folder_coverage: the store count bounds the IMAP count\n'

# himalaya answers the window listing with uids 1..$PROBE_IMAP_IDS and an
# empty \Deleted listing; auberge answers with a verdict whose store count is
# $PROBE_STORE, whose undated store uids are $PROBE_UNDATED (a JSON array),
# and whose identity status is $PROBE_STATUS.
# shellcheck disable=SC2329  # invoked indirectly by probe_folder_coverage in the sourced script
himalaya() {
  if [[ " $* " == *' --deleted '* ]]; then
    printf '{"ids":[],"uid_mode":true}'
    return 0
  fi
  local ids
  ids=$(seq -s, 1 "${PROBE_IMAP_IDS}" | sed 's/[0-9]\+/{"id":&}/g')
  printf '{"ids":[%s],"uid_mode":true}' "${ids}"
}
# shellcheck disable=SC2329  # invoked indirectly by probe_folder_coverage in the sourced script
auberge() {
  printf '{"status":"%s","store_messages":%s,"store_internal_date_count":%s,"store_without_internal_date":%s,"missing":[],"unverifiable":{"store_synthetic":0,"archive_sha256":0,"contested":[]}}' \
    "${PROBE_STATUS}" "${PROBE_STORE}" "${PROBE_STORE}" "${PROBE_UNDATED:-[]}"
}

probe_status_for() {
  PROBE_IMAP_IDS="$1" PROBE_STORE="$2" PROBE_STATUS="$3" PROBE_UNDATED="${4:-[]}"
  # shellcheck disable=SC2034  # intentional: read by probe_folder_coverage in the sourced script
  account='a@x.com' folder='Archive' host='h' window_days=90 archive_path='/x'
  probe_folder_coverage 2>/dev/null
  printf '%s' "${PAIR_STATUS}"
}

# dev@ Archive on auberge (ADR-0088): the server holds one message more than
# the store ever ingested.
assert_eq 'more IMAP messages than store messages is a gap' 'gap' \
  "$(probe_status_for 442 441 covered)"

# me@ Archive: 294 on both sides, 287 distinct ids. Repeated ids no longer
# enter the count, so equal raw counts pass.
assert_eq 'equal raw counts pass the count gate' 'ok' \
  "$(probe_status_for 294 294 covered)"

assert_eq 'a store holding more than the server passes the count gate' 'ok' \
  "$(probe_status_for 3 5 covered)"

assert_eq 'a passed count still answers to identity' 'identity-gap' \
  "$(probe_status_for 3 3 gap)"

# The count gate speaks first: an un-ingested message is the root cause an
# identity gap would only echo.
assert_eq 'the count gap outranks an identity gap' 'gap' \
  "$(probe_status_for 4 3 gap)"

gap_finding() {
  (
    probe_status_for 442 441 covered >/dev/null
    probe_folder_coverage 2>/dev/null
    render_pair_verdict_or_die
  ) 2>&1
}
assert_succeeds 'the single-target refusal names how many the store has not ingested' \
  contains "$(gap_finding)" '1 message(s) in Archive are on the server but not in the Internal Store (IMAP 442, store 441)'

sweep_gap_row() {
  SWEEP_ROWS=()
  PROBE_IMAP_IDS=442 PROBE_STORE=441 PROBE_STATUS=covered
  # shellcheck disable=SC2034  # intentional: read by sweep_probe_pair in the sourced script
  account='a@x.com' folder='Archive' host='h' window_days=90 archive_path='/x'
  sweep_probe_pair 2>/dev/null
  printf '%s' "${SWEEP_ROWS[0]}"
}
assert_eq 'the sweep records the un-ingested count as a finding' \
  $'finding\ta@x.com\tArchive\t0\t1 message(s) on the server not ingested by the store (IMAP 442, store 441)' \
  "$(sweep_gap_row)"

# me@ Archive on auberge: 294 on both sides, 3 store messages with internal
# date 0. The store counts them inside the window; that is only true if the
# server's own SEARCH BEFORE returned their uids.
assert_eq 'undated store uids inside the IMAP window pass' 'ok' \
  "$(probe_status_for 294 294 covered '[2,100,294]')"

assert_eq 'an undated store uid outside the IMAP window refuses' 'undated' \
  "$(probe_status_for 294 294 covered '[2,900]')"

# A gap already refuses; an inflated store only hides more, never less.
assert_eq 'the count gap outranks an undated uid' 'gap' \
  "$(probe_status_for 294 293 covered '[900]')"

undated_finding() {
  (
    probe_status_for 294 294 covered '[2,900,901]' >/dev/null
    probe_folder_coverage 2>/dev/null
    render_pair_verdict_or_die
  ) 2>&1
}
assert_succeeds 'the single-target refusal names the undated uids outside the window' \
  contains "$(undated_finding)" '2 store message(s) in Archive carry no internal date and are not in the IMAP window: uids 900 901'

sweep_undated_row() {
  SWEEP_ROWS=()
  PROBE_IMAP_IDS=294 PROBE_STORE=294 PROBE_STATUS=covered PROBE_UNDATED='[900]'
  # shellcheck disable=SC2034  # intentional: read by sweep_probe_pair in the sourced script
  account='a@x.com' folder='Archive' host='h' window_days=90 archive_path='/x'
  sweep_probe_pair 2>/dev/null
  printf '%s' "${SWEEP_ROWS[0]}"
}
assert_eq 'the sweep records undated uids outside the window as a finding' \
  $'finding\ta@x.com\tArchive\t0\tundated store uid(s) 900 not in the IMAP window — the store count cannot be trusted' \
  "$(sweep_undated_row)"
unset -f himalaya auberge

printf '\n== collect_envelope_ids\n'

ENVELOPE_JSON='{"ids":[{"id":1},{"id":2},{"id":3}],"uid_mode":true}'
IMAP_COUNT=3
assert_succeeds 'a clean listing collects its ids' collect_envelope_ids
assert_eq 'the ids survive in order' '1 2 3' "${ENVELOPE_IDS[*]}"

# The listing gate 3 counted is the only thing the expunge may act on; a
# diverging id count means it changed underfoot.
IMAP_COUNT=2
assert_fails 'a count mismatch refuses the listing' collect_envelope_ids
assert_succeeds 'the mismatch is named' contains "${IDS_ERROR}" 'do not match'

# `imap store` reads its set as UIDs, so message numbers would flag other mail.
ENVELOPE_JSON='{"ids":[{"id":1},{"id":2},{"id":3}],"uid_mode":false}' IMAP_COUNT=3
assert_fails 'message numbers are refused' collect_envelope_ids
assert_succeeds 'the refusal names UIDs' contains "${IDS_ERROR}" 'not UIDs'

# A stray `*` would widen the comma-joined set into a range.
# shellcheck disable=SC2034  # intentional: read by collect_envelope_ids in the sourced script
ENVELOPE_JSON='{"ids":[{"id":1},{"id":"*"}],"uid_mode":true}' IMAP_COUNT=2
assert_fails 'a non-numeric id refuses the listing' collect_envelope_ids
assert_succeeds 'the offending id is named' contains "${IDS_ERROR}" 'non-numeric'

printf '\n== himalaya_account_names\n'

# shellcheck disable=SC2034  # intentional: read by himalaya_account_names in the sourced script
HIMALAYA_ACCOUNTS_JSON='{"accounts":[{"name":"a@x.com","default":true,"backends":["imap"]},{"name":"b@x.com","default":false,"backends":["imap"]}]}'
assert_eq 'v2 account list yields the names' $'a@x.com\nb@x.com' "$(himalaya_account_names)"

printf '\n== fetch_account_folders: INBOX spelling\n'

# shellcheck disable=SC2329  # invoked indirectly by himalaya_json in the sourced script
himalaya() {
  printf '%s' '{"mailboxes":[{"name":"Archive"},{"name":"INBOX.spam"},{"name":"Inbox"},{"name":"Sent"}]}'
}
account='a@x.com'
fetch_account_folders
assert_eq 'any spelling of inbox becomes INBOX; other names pass through' \
  $'Archive\nINBOX\nINBOX.spam\nSent' "${FOLDER_NAMES}"
unset -f himalaya

printf '\n== typed_report\n'

assert_eq 'a carriage return is shown, not swallowed' "received: \$'auberge\\r'" "$(typed_report $'auberge\r')"
assert_eq 'bracketed-paste markers are shown' "received: \$'\\E[200~auberge\\E[201~'" "$(typed_report $'\e[200~auberge\e[201~')"

printf '\n== drain_typeahead\n'

drained_then_read() {
  printf 'stray\n\e[A' | {
    drain_typeahead
    IFS= read -r line && printf 'left:%s' "${line}" || printf 'empty'
  }
}
assert_eq 'input buffered before the prompt is discarded' 'empty' "$(drained_then_read)"

printf '\n== expunge_ids: batched STORE, one EXPUNGE\n'

# Stub himalaya to record each call's mailbox-scoped tail; step writes to stderr.
HIMALAYA_CALLS=''
# shellcheck disable=SC2329  # invoked indirectly by expunge_ids in the sourced script
himalaya() {
  HIMALAYA_CALLS+="${*: -1}"$'\n'
}

expunge_calls_for() {
  HIMALAYA_CALLS=''
  expunge_ids 'a@x.com' 'INBOX' "$@" 2>/dev/null
  printf '%s' "${HIMALAYA_CALLS}"
}

mapfile -t MANY_IDS < <(seq 1 1001)
assert_eq 'ids are stored in batches of STORE_BATCH_SIZE, then one expunge' \
  "$(seq -s, 1 500)"$'\n'"$(seq -s, 501 1000)"$'\n1001\nINBOX' \
  "$(expunge_calls_for "${MANY_IDS[@]}")"

# shellcheck disable=SC2329  # invoked indirectly by expunge_ids in the sourced script
himalaya() {
  [[ "$2" == store ]] && return 1
  return 0
}
expunge_rc() {
  local rc=0
  expunge_ids 'a@x.com' 'INBOX' 1 2 2>/dev/null || rc=$?
  printf '%s' "${rc}"
}
assert_eq 'a failed STORE returns 1 before any expunge' '1' "$(expunge_rc)"

# --flag takes several values, so without `--` the uid set is read as a second
# flag and himalaya refuses for want of a SEQUENCE.
# shellcheck disable=SC2329  # invoked indirectly by expunge_ids in the sourced script
himalaya() {
  [[ "$2" == store ]] && STORE_ARGS="$*"
  return 0
}
STORE_ARGS=''
expunge_ids 'a@x.com' 'INBOX' 7 9 2>/dev/null
assert_eq 'the uid set follows -- so --flag cannot swallow it' \
  'imap store --account a@x.com --mailbox INBOX --flag \Deleted -- 7,9' "${STORE_ARGS}"

# shellcheck disable=SC2329  # invoked indirectly by expunge_ids in the sourced script
himalaya() {
  [[ "$2" == expunge ]] && return 1
  return 0
}
assert_eq 'a failed EXPUNGE after the flags returns 2' '2' "$(expunge_rc)"
unset -f himalaya

printf '\n== sweep findings carry the reason (#1313)\n'

# A sweep prints each pair's stderr once, mid-scroll; the summary row is what the
# operator reads. A generic row left them guessing at throttling when the server
# had said "Authentication failed".
readonly AUTH_FAILED='{"error":"IMAP AUTHENTICATE PLAIN failed: NO Authentication failed.","sources":[],"backtrace":null}'

last_sweep_detail() {
  local row="${SWEEP_ROWS[-1]}"
  printf '%s' "${row##*$'\t'}"
}

# shellcheck disable=SC2034  # intentional: read by the sourced script's functions
account='a@x.com' folder='INBOX' window_days=90 host='h' archive_path='/x'

# shellcheck disable=SC2329  # invoked indirectly by himalaya_json in the sourced script
himalaya() {
  printf '%s' "${AUTH_FAILED}"
  return 1
}
SWEEP_ROWS=()
sweep_probe_pair 2>/dev/null
assert_eq 'a failed window listing names the server reply' \
  'himalaya could not list the folder: IMAP AUTHENTICATE PLAIN failed: NO Authentication failed.' \
  "$(last_sweep_detail)"

SWEEP_ROWS=()
sweep_probe_account 2>/dev/null
assert_eq 'a failed folder listing names the server reply' \
  'himalaya could not list the folders of a@x.com: IMAP AUTHENTICATE PLAIN failed: NO Authentication failed.' \
  "$(last_sweep_detail)"

assert_eq 'a multi-line reason stays on the one row' ': caused by: a b' \
  "$(himalaya_reason '{"error":"caused by:\n\ta\nb"}')"

# shellcheck disable=SC2329  # invoked indirectly by himalaya_json in the sourced script
himalaya() {
  case "$*" in
    *--before*) printf '%s' '{"ids":[{"id":7}],"uid_mode":true}' ;;
    *)
      printf '%s' "${AUTH_FAILED}"
      return 1
      ;;
  esac
}
SWEEP_ROWS=()
sweep_probe_pair 2>/dev/null
assert_eq 'a failed deleted-flag listing names the server reply' \
  'himalaya could not list deleted-flagged mail: IMAP AUTHENTICATE PLAIN failed: NO Authentication failed.' \
  "$(last_sweep_detail)"

# shellcheck disable=SC2329  # invoked indirectly by himalaya_json in the sourced script
himalaya() {
  case "$*" in
    *--before*) printf '%s' '{"ids":[{"id":7}],"uid_mode":true}' ;;
    *) printf '%s' '{"ids":[],"uid_mode":true}' ;;
  esac
}
# shellcheck disable=SC2329  # invoked by probe_folder_coverage in the sourced script
auberge() {
  printf '⚠ verify-coverage failed: ssh: connect to host h port 22: Connection timed out\n' >&2
  return 2
}
SWEEP_ROWS=()
assert_eq 'auberge still says why on stderr' \
  '⚠ verify-coverage failed: ssh: connect to host h port 22: Connection timed out' \
  "$(sweep_probe_pair 2>&1 >/dev/null | grep -F 'verify-coverage failed')"
sweep_probe_pair 2>/dev/null
assert_eq 'a failed identity verdict names auberge'"'"'s reason' \
  'auberge could not produce an identity verdict: verify-coverage failed: ssh: connect to host h port 22: Connection timed out' \
  "$(last_sweep_detail)"
unset -f himalaya auberge

printf '\n== validate_options: --account count and --sweep/--folder exclusion\n'

# validate_options calls die, which exits — same subshell shape as
# synced_verdict above. $1 is --sweep, $2 --folder, the rest each --account.
options_ok() {
  local sweep_flag="$1" folder_flag="$2"
  shift 2
  (
    # shellcheck disable=SC2030,SC2034  # intentional: read by validate_options in the sourced script
    sweep="${sweep_flag}" folder="${folder_flag}" window_days=90 archive_path='/x'
    # shellcheck disable=SC2034  # intentional: read by validate_options in the sourced script
    requested_accounts=("$@")
    validate_options
  ) >/dev/null 2>&1
}

assert_fails '--sweep refuses a passed --folder' options_ok true 'INBOX'
assert_succeeds '--sweep alone passes option validation' options_ok true ''

# The point of the change: a sweep narrowed to accounts is not contradictory
# intent, because --account touches the account set and never the folder set.
assert_succeeds '--sweep narrows to one account' options_ok true '' 'a@x.com'
assert_succeeds '--sweep narrows to several accounts' options_ok true '' 'a@x.com' 'b@x.com'
assert_fails '--sweep still refuses --folder beside --account' options_ok true 'INBOX' 'a@x.com'

assert_succeeds 'one target takes one --account' options_ok false '' 'a@x.com'
assert_fails 'one target refuses two --account' options_ok false '' 'a@x.com' 'b@x.com'
assert_succeeds 'no --account passes; the menu or the sweep supplies it' options_ok false ''

assert_fails 'an empty --account is refused' options_ok false '' ''
assert_fails 'a whitespace --account is refused in a sweep too' options_ok true '' '   '

printf '\n== resolve_account\n'

# Same subshell shape: the no-account branch dies non-interactively (no_input
# is still true from the resolve_folder section above).
resolve_account_in() {
  (
    # shellcheck disable=SC2030,SC2034  # intentional: read by resolve_account in the sourced script
    requested_accounts=("$@")
    account=''
    resolve_account
    printf '%s' "${account}"
  ) 2>/dev/null
}

assert_eq 'a passed account resolves to itself' 'a@x.com' \
  "$(resolve_account_in 'a@x.com')"

assert_fails 'no --account and no TTY refuses to guess' resolve_account_in

report 'bichon-expunge'
