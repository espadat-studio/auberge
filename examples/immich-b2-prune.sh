#!/usr/bin/env bash
#
# examples/immich-b2-prune.sh
#
# REFERENCE SCRIPT — NOT SHIPPED IN THE auberge BINARY
# ---------------------------------------------------
# The laptop half of the Immich offsite backup (#558). The Immich Host backs
# up nightly to B2 with a crippled application key that cannot delete, so
# retention has to run where the full key lives: here, weekly, as a systemd
# user timer.
#
# Two phases, in order:
#
#   1. watchdog   newest snapshot older than 48h — or an empty or unreadable
#                 repository — raises a desktop notification and exits 1, a
#                 failed user unit. Retention does not run: pruning a repo the
#                 box has stopped writing to only walks toward destroying the
#                 history that is left. Age is read from a local ledger, not
#                 from the time the box wrote into the snapshot: the box is
#                 what this half guards against. A new snapshot dated in the
#                 future, or older than the newest one an earlier run already
#                 saw, raises the same alarm, stays out of the ledger so the
#                 alarm repeats, and blocks retention — --keep-within and
#                 --keep-monthly both count from snapshot times, so a forged
#                 time would steer which genuine snapshots get forgotten.
#   2. retention  restic forget --keep-within 30d --keep-monthly 12 --prune,
#                 per restic's append-only guidance.
#
# Configuration — the environment is the whole surface. restic reads its own
# variables: RESTIC_REPOSITORY, RESTIC_PASSWORD (or RESTIC_PASSWORD_COMMAND /
# RESTIC_PASSWORD_FILE), and whatever the backend needs — for B2's S3 API,
# AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY. This script assumes an offsite
# repository already exists and the environment can reach it; it deliberately
# knows nothing about any secret store or config tool. The credentials it runs
# with must be the FULL key — the one with deleteFiles — never the box key:
# retention against the box key fails, which is exactly the capability split
# #558 exists to prove. The 48h threshold is overridable via
# IMMICH_SNAPSHOT_MAX_AGE_HOURS. The ledger lives at
# ${XDG_STATE_HOME:-~/.local/state}/immich-b2-prune/ledger, overridable via
# IMMICH_PRUNE_LEDGER; one line per snapshot: id, first-seen epoch, claimed
# epoch. First-seen is capped at the claimed time, so a weekly run does not
# make a snapshot written six days ago look fresh.
#
# Install on the laptop — a wrapper owns provisioning the environment, so the
# unit file holds no credentials and this script stays tool-agnostic:
#
#   cp examples/immich-b2-prune.sh ~/.local/bin/immich-b2-prune
#
#   ~/.local/bin/immich-b2-prune-wrapped (0700), adapted to your own tooling:
#     #!/usr/bin/env bash
#     set -euo pipefail
#     export RESTIC_REPOSITORY='s3:https://s3.example.com/my-immich-bucket'
#     export RESTIC_PASSWORD="$(your-secret-tool get immich-restic-password)"
#     export AWS_ACCESS_KEY_ID="$(your-secret-tool get immich-b2-key-id)"
#     export AWS_SECRET_ACCESS_KEY="$(your-secret-tool get immich-b2-key)"
#     exec "${HOME}/.local/bin/immich-b2-prune"
#
#   ~/.config/systemd/user/immich-b2-prune.service:
#     [Unit]
#     Description=Immich B2 retention and staleness watchdog
#     [Service]
#     Type=oneshot
#     ExecStart=%h/.local/bin/immich-b2-prune-wrapped
#
#   ~/.config/systemd/user/immich-b2-prune.timer:
#     [Unit]
#     Description=Run the Immich B2 retention weekly
#     [Timer]
#     OnCalendar=weekly
#     RandomizedDelaySec=1h
#     Persistent=true
#     [Install]
#     WantedBy=timers.target
#
#   systemctl --user daemon-reload
#   systemctl --user enable --now immich-b2-prune.timer
#
# Verify the watchdog once by pointing RESTIC_REPOSITORY at an empty or stale
# repository and watching the run fail with a notification.
#
# Prerequisites: restic, jq, GNU date.
#
# Exit codes:
#   0 — backup fresh, retention applied
#   1 — watchdog finding (stale, empty, or forged-time repository) or
#       retention failed
#   2 — operational error: missing prerequisite, credential resolution failed,
#       the repository or the ledger could not be read (distinguishable from "found
#       stale", the way the fleet's other watchers keep the two apart)
#
# shellcheck shell=bash

set -euo pipefail

readonly PROGRAM_NAME="${0##*/}"
readonly KEEP_WITHIN='30d'
readonly KEEP_MONTHLY=12
readonly MAX_SNAPSHOT_AGE_HOURS="${IMMICH_SNAPSHOT_MAX_AGE_HOURS:-48}"
readonly MAX_CLOCK_SKEW_SECONDS=3600
readonly LEDGER="${IMMICH_PRUNE_LEDGER:-${XDG_STATE_HOME:-${HOME}/.local/state}/immich-b2-prune/ledger}"

# die <exit-code> <message> [remediation-line...]
die() {
  local code="$1" message="$2"
  shift 2
  printf '%s: error: %s\n' "${PROGRAM_NAME}" "${message}" >&2
  local line
  for line in "$@"; do
    printf '  %s\n' "${line}" >&2
  done
  exit "${code}"
}

# The failed user unit is the durable signal; the notification is the one a
# desktop actually surfaces. Its delivery must not mask the message, so it is
# best-effort by design.
alert() {
  local message="$1"
  if command -v notify-send >/dev/null 2>&1; then
    notify-send --urgency=critical 'immich offsite backup' "${message}" || true
  fi
  printf '%s: %s\n' "${PROGRAM_NAME}" "${message}" >&2
}

check_prerequisites() {
  local tool missing=()
  for tool in restic jq date; do
    command -v "${tool}" >/dev/null 2>&1 || missing+=("${tool}")
  done
  ((${#missing[@]} == 0)) \
    || die 2 "not on PATH: ${missing[*]}" \
      'see the Prerequisites block at the top of this script'

  [[ "${MAX_SNAPSHOT_AGE_HOURS}" =~ ^[1-9][0-9]*$ ]] \
    || die 2 "IMMICH_SNAPSHOT_MAX_AGE_HOURS must be a positive integer, got: ${MAX_SNAPSHOT_AGE_HOURS}"
}

# Which tool provisions the environment is the wrapper's business (see the
# Install block above); guessing one here would quietly couple every operator
# to it. Only the repository location is checked — restic's own complaint for
# it is cryptic, while a missing password or backend credential varies by
# backend and surfaces through the watchdog as an unreadable repository.
require_repository() {
  [[ -n "${RESTIC_REPOSITORY:-}" ]] \
    || die 2 'RESTIC_REPOSITORY must be set in the environment' \
      'this script assumes an offsite restic repository already exists;' \
      'the wrapper that launches it provisions the environment (see Install' \
      'in the header)'
}

watchdog() {
  local snapshots_json
  # exit 2, not 1: an unreadable repository is an operational error, not a
  # staleness finding — but it blocks retention all the same, and the operator
  # still gets woken up.
  if ! snapshots_json=$(restic snapshots --json); then
    alert 'cannot read the B2 repository — the offsite backup cannot be vouched for'
    exit 2
  fi

  local listing
  listing=$(printf '%s' "${snapshots_json}" | jq -r '.[] | [.id, .time] | @tsv') \
    || die 2 'restic snapshots returned JSON this script cannot read'

  if [[ -z "${listing}" ]]; then
    alert 'the B2 repository holds no snapshots — the Immich box has never backed up'
    exit 1
  fi

  local -A first_seen=()
  local last_newest=0 id seen claimed
  if [[ -e "${LEDGER}" ]]; then
    while IFS=$'\t' read -r id seen claimed; do
      [[ "${id}" =~ ^[0-9a-f]{64}$ && "${seen}" =~ ^[0-9]+$ && "${claimed}" =~ ^[0-9]+$ ]] \
        || die 2 "ledger is corrupt: ${LEDGER}" \
          'it is the only record of when snapshots really appeared; restore it' \
          'from backup, or delete it to re-trust the snapshot times once'
      first_seen["${id}"]="${seen}"
      if ((claimed > last_newest)); then last_newest="${claimed}"; fi
    done <"${LEDGER}"
  fi

  local now time newest=0 rows=() forged=()
  now=$(date +%s)
  while IFS=$'\t' read -r id time; do
    [[ "${id}" =~ ^[0-9a-f]{64}$ ]] \
      || die 2 "restic listed a snapshot id this script cannot read: ${id}"
    claimed=$(date -d "${time}" +%s) \
      || die 2 "cannot parse snapshot time: ${time}"
    if [[ -n "${first_seen[${id}]+set}" ]]; then
      seen="${first_seen[${id}]}"
    elif ((claimed > now + MAX_CLOCK_SKEW_SECONDS)); then
      forged+=("${id:0:8} is dated ${time}, in the future")
      continue
    elif ((claimed < last_newest)); then
      forged+=("${id:0:8} is dated ${time}, before snapshots an earlier run already saw")
      continue
    else
      seen=$((claimed < now ? claimed : now))
    fi
    rows+=("${id}"$'\t'"${seen}"$'\t'"${claimed}"$'\n')
    if ((seen > newest)); then newest="${seen}"; fi
  done <<<"${listing}"

  mkdir -p -- "$(dirname -- "${LEDGER}")"
  printf '%s' "${rows[@]}" >"${LEDGER}.tmp"
  mv -f -- "${LEDGER}.tmp" "${LEDGER}"

  if ((${#forged[@]} > 0)); then
    alert "new snapshot times cannot be trusted, retention skipped: ${forged[*]}"
    exit 1
  fi

  local age_hours=$(((now - newest) / 3600))
  if ((age_hours > MAX_SNAPSHOT_AGE_HOURS)); then
    alert "newest snapshot is ${age_hours}h old (limit ${MAX_SNAPSHOT_AGE_HOURS}h) — the Immich box has stopped backing up"
    exit 1
  fi
}

apply_retention() {
  restic forget --keep-within "${KEEP_WITHIN}" --keep-monthly "${KEEP_MONTHLY}" --prune \
    || die 1 'retention failed' \
      'a stale lock from an interrupted run is the usual cause: restic unlock' \
      'a permission error means the box key leaked into this half — it must' \
      'never be able to prune'
}

main() {
  check_prerequisites
  require_repository
  watchdog
  apply_retention
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
