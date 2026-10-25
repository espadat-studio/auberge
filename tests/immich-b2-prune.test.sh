#!/usr/bin/env bash
#
# tests/immich-b2-prune.test.sh
#
# Unit tests for the laptop half of the Immich offsite backup (#558): the
# weekly watchdog + retention script that holds the only key allowed to
# destroy repository data.
#
# No restic, no B2 — stubs on PATH whose behavior each case stages (jq and
# date are real). The properties worth pinning are the ones the append-only
# posture depends on: that a stale, empty, or unreadable repository raises the
# alarm and blocks retention (pruning a repo the box has stopped writing to
# would destroy the only history left), and that the whole configuration
# surface is the environment — the script deliberately knows nothing about any
# secret store or config tool.
#
# Run: ./tests/immich-b2-prune.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly SCRIPT="${REPO_ROOT}/examples/immich-b2-prune.sh"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

readonly BIN="${WORK}/bin"
readonly RESTIC_ARGS="${WORK}/restic.args"
readonly SNAPSHOTS_OUT="${WORK}/snapshots.out"
readonly SNAPSHOTS_RC="${WORK}/snapshots.rc"
readonly FORGET_RC="${WORK}/forget.rc"
readonly FORGET_SEEN_PASSWORD="${WORK}/forget.seen-password"
readonly FORGET_SEEN_AWS_KEY="${WORK}/forget.seen-aws-key"
readonly NOTIFY_ARGS="${WORK}/notify.args"
readonly LEDGER="${WORK}/ledger"
readonly PLAN_OUT="${WORK}/plan.out"

mkdir -p "${BIN}"

cat >"${BIN}/restic" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${RESTIC_ARGS}"
case "\$1" in
  snapshots)
    rc="\$(cat "${SNAPSHOTS_RC}")"
    if [ "\${rc}" -ne 0 ]; then exit "\${rc}"; fi
    cat "${SNAPSHOTS_OUT}"
    ;;
  forget)
    if [ "\$2" = --dry-run ]; then
      cat "${PLAN_OUT}"
      exit 0
    fi
    printf '%s\n' "\${RESTIC_PASSWORD:-}" >"${FORGET_SEEN_PASSWORD}"
    printf '%s\n' "\${AWS_SECRET_ACCESS_KEY:-}" >"${FORGET_SEEN_AWS_KEY}"
    exit "\$(cat "${FORGET_RC}")"
    ;;
esac
STUB
chmod 0755 "${BIN}/restic"

cat >"${BIN}/notify-send" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"${NOTIFY_ARGS}"
STUB
chmod 0755 "${BIN}/notify-send"

export PATH="${BIN}:${PATH}"

snapshot_id() {
  printf '%s' "$1" | sha256sum | cut -c1-64
}

# list <snapshot-time…>
# Each snapshot time becomes one entry in the staged `restic snapshots --json`
# listing, its id derived from the time so a re-listed snapshot keeps its id;
# none stages an empty repository. The staged retention plan removes the first
# snapshot and keeps the rest.
list() {
  local time entries=()
  for time in "$@"; do
    entries+=("{\"id\": \"$(snapshot_id "${time}")\", \"time\": \"${time}\"}")
  done
  printf '[%s]\n' "$(
    IFS=,
    printf '%s' "${entries[*]-}"
  )" >"${SNAPSHOTS_OUT}"
  jq 'if length > 1 then [{keep: .[1:], remove: .[:1]}] else [{keep: ., remove: null}] end' \
    "${SNAPSHOTS_OUT}" >"${PLAN_OUT}"
  : >"${RESTIC_ARGS}"
  : >"${NOTIFY_ARGS}"
}

# stage <snapshots-rc> <forget-rc> <snapshot-time…>
# A laptop whose earlier runs saw nothing yet.
stage() {
  local snapshots_rc="$1" forget_rc="$2"
  shift 2
  printf '%s' "${snapshots_rc}" >"${SNAPSHOTS_RC}"
  printf '%s' "${forget_rc}" >"${FORGET_RC}"
  : >"${LEDGER}"
  list "$@"
  : >"${FORGET_SEEN_PASSWORD}"
  : >"${FORGET_SEEN_AWS_KEY}"
}

# The environment is the whole configuration surface — provisioning it is the
# wrapper's job in a real install.
run_prune() {
  local status=0
  env \
    RESTIC_REPOSITORY='s3:https://s3.example.test/immich' \
    RESTIC_PASSWORD='repo-pass' \
    AWS_ACCESS_KEY_ID='full-key-id' \
    AWS_SECRET_ACCESS_KEY='full-key-secret' \
    IMMICH_PRUNE_LEDGER="${LEDGER}" \
    "${SCRIPT}" >/dev/null 2>&1 || status=$?
  printf '%s' "${status}"
}

hours_ago() {
  date -u -d "-$1 hours" '+%Y-%m-%dT%H:%M:%SZ'
}

hours_from_now() {
  date -u -d "+$1 hours" '+%Y-%m-%dT%H:%M:%SZ'
}

# ── fresh repository: retention runs, nobody is woken up ──

older="$(hours_ago 60)"
stage 0 0 "${older}" "$(hours_ago 6)"
assert_eq 'fresh snapshot exits 0' '0' "$(run_prune)"
assert_eq 'watchdog reads the repo, retention plans, then prunes by id' \
  "snapshots --json
forget --dry-run --json --keep-within 30d --keep-monthly 12
forget --prune $(snapshot_id "${older}")" \
  "$(cat "${RESTIC_ARGS}")"
assert_eq 'no notification when the backup is healthy' '' "$(cat "${NOTIFY_ARGS}")"
assert_eq 'restic sees the repository password from the environment' \
  'repo-pass' "$(cat "${FORGET_SEEN_PASSWORD}")"
assert_eq 'restic sees the application key from the environment' \
  'full-key-secret' "$(cat "${FORGET_SEEN_AWS_KEY}")"

# ── stale repository: alarm, and retention must not run ──

stage 0 0 "$(hours_ago 100)"
assert_eq 'stale snapshot exits 1' '1' "$(run_prune)"
assert_eq 'stale snapshot raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'stale snapshot blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"

# ── empty repository: same alarm — there is nothing safe to prune ──

stage 0 0
assert_eq 'empty repository exits 1' '1' "$(run_prune)"
assert_eq 'empty repository raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'empty repository blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"

# ── unreadable repository: an operational error, not a staleness finding ──

stage 1 0 "$(hours_ago 6)"
assert_eq 'unreadable repository exits 2' '2' "$(run_prune)"
assert_eq 'unreadable repository raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'unreadable repository blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"

# ── a missing repository refuses the run before restic — no tool guessing ──

stage 0 0 "$(hours_ago 6)"
status=0
"${SCRIPT}" >/dev/null 2>&1 || status=$?
assert_eq 'missing RESTIC_REPOSITORY exits 2' '2' "${status}"
assert_eq 'missing RESTIC_REPOSITORY touches restic not at all' '' "$(cat "${RESTIC_ARGS}")"

# ── a failed forget fails the unit ──

stage 0 1 "$(hours_ago 60)" "$(hours_ago 6)"
assert_eq 'failed forget exits 1' '1' "$(run_prune)"

# ── nothing to remove: no prune call ──

stage 0 0 "$(hours_ago 6)"
assert_eq 'nothing to remove exits 0' '0' "$(run_prune)"
assert_eq 'nothing to remove skips the prune call' \
  "snapshots --json
forget --dry-run --json --keep-within 30d --keep-monthly 12" \
  "$(cat "${RESTIC_ARGS}")"

# ── a snapshot written between the watchdog and the plan blocks retention ──

stage 0 0 "$(hours_ago 60)" "$(hours_ago 6)"
late="$(hours_from_now 400)"
jq --arg id "$(snapshot_id "${late}")" '.[0].keep += [{id: $id}]' "${PLAN_OUT}" >"${PLAN_OUT}.new"
mv "${PLAN_OUT}.new" "${PLAN_OUT}"
assert_eq 'unvetted snapshot in the plan exits 1' '1' "$(run_prune)"
assert_eq 'unvetted snapshot in the plan raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'unvetted snapshot in the plan blocks the prune' \
  "snapshots --json
forget --dry-run --json --keep-within 30d --keep-monthly 12" \
  "$(cat "${RESTIC_ARGS}")"

# ── no ledger: trusted once, loudly ──

stage 0 0 "$(hours_ago 6)"
rm -f "${LEDGER}"
assert_eq 'missing ledger still runs' '0' "$(run_prune)"
assert_eq 'missing ledger raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'missing ledger is recreated' '1' "$(wc -l <"${LEDGER}")"

# ── the ledger records every snapshot it vouches for ──

older="$(hours_ago 60)" newer="$(hours_ago 6)"
stage 0 0 "${older}" "${newer}"
run_prune >/dev/null
assert_eq 'first run records every listed snapshot' \
  "$(printf '%s\n' "$(snapshot_id "${older}")" "$(snapshot_id "${newer}")" | sort)" \
  "$(cut -f1 "${LEDGER}" | sort)"

# ── a backdated new snapshot: alarm, retention skipped, not recorded ──

seen="$(hours_ago 6)" backdated="$(hours_ago 30)"
stage 0 0 "${seen}"
run_prune >/dev/null
list "${seen}" "${backdated}"
assert_eq 'backdated snapshot exits 1' '1' "$(run_prune)"
assert_eq 'backdated snapshot raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'backdated snapshot blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"
assert_eq 'backdated snapshot is recorded as rejected' '1' \
  "$(grep -c "^$(snapshot_id "${backdated}")"$'\t-\t' "${LEDGER}")"
list "${seen}" "${backdated}"
assert_eq 'backdated snapshot keeps alarming on the next run' '1' "$(run_prune)"

# ── a future-dated new snapshot: alarm, retention skipped, not recorded ──

future="$(hours_from_now 48)"
stage 0 0 "$(hours_ago 6)" "${future}"
assert_eq 'future-dated snapshot exits 1' '1' "$(run_prune)"
assert_eq 'future-dated snapshot raises a desktop notification' '1' \
  "$(grep -c 'immich' "${NOTIFY_ARGS}")"
assert_eq 'future-dated snapshot blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"
assert_eq 'future-dated snapshot is recorded as rejected' '1' \
  "$(grep -c "^$(snapshot_id "${future}")"$'\t-\t' "${LEDGER}")"
passed="$(hours_ago 1)"
stage 0 0 "$(hours_ago 6)" "${passed}"
printf '%s\t-\t%s\n' "$(snapshot_id "${passed}")" "$(date -d "${passed}" +%s)" >"${LEDGER}"
assert_eq 'rejected snapshot keeps alarming once its date has passed' '1' "$(run_prune)"

# ── freshness comes from the ledger, not the snapshot's own claim ──

claimed="$(hours_ago 6)"
stage 0 0 "${claimed}"
printf '%s\t%s\t%s\n' "$(snapshot_id "${claimed}")" \
  "$(date -d "$(hours_ago 100)" +%s)" "$(date -d "${claimed}" +%s)" >"${LEDGER}"
assert_eq 'snapshot first seen 100h ago is stale whatever it claims' '1' "$(run_prune)"
assert_eq 'stale first-seen blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"

# ── first-seen is capped at the claimed time: a late-noticed stale box is stale ──

stage 0 0 "$(hours_ago 100)"
run_prune >/dev/null
list "$(hours_ago 100)" "$(hours_ago 60)"
assert_eq 'new snapshot claimed 60h ago is stale though first seen now' '1' "$(run_prune)"

# ── the backdating baseline survives snapshots leaving the repository ──

seen="$(hours_ago 6)" backdated="$(hours_ago 30)"
stage 0 0 "${seen}"
run_prune >/dev/null
list "${backdated}"
assert_eq 'backdated snapshot alarms after the newer one is gone' '1' "$(run_prune)"

# ── a ledger that cannot be read is an operational error ──

stage 0 0 "$(hours_ago 6)"
chmod 0000 "${LEDGER}"
assert_eq 'unreadable ledger exits 2' '2' "$(run_prune)"
chmod 0600 "${LEDGER}"

# ── a corrupt ledger is an operational error ──

stage 0 0 "$(hours_ago 6)"
printf 'not a ledger\n' >"${LEDGER}"
assert_eq 'corrupt ledger exits 2' '2' "$(run_prune)"
assert_eq 'corrupt ledger blocks retention' \
  'snapshots --json' "$(cat "${RESTIC_ARGS}")"

report 'immich-b2-prune'
