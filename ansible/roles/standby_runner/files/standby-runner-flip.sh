#!/usr/bin/env bash
#
# The Runner Flip (ADR-0085). Finds failed runs that GitHub refused to start for
# billing, points CI_RUNNER at the Standby Runner in that repo, and re-runs the
# failed jobs once. On UTC day 1 it clears CI_RUNNER everywhere instead, since
# the month's minutes have reset.
#
# A billing failure is a job with zero steps, no runner name, and the billing
# annotation. Anything else, an ordinary test failure included, is only marked
# seen: never re-run, never a flip trigger.

set -euo pipefail

: "${STANDBY_RUNNER_APP_ID:?}" "${STANDBY_RUNNER_KEY_PATH:?}"
: "${STANDBY_RUNNER_LABEL:?}" "${STANDBY_RUNNER_STATE_DIR:?}"

# shellcheck source=./standby-runner-lib.sh disable=SC1091
source "${STANDBY_RUNNER_LIB:-/usr/local/lib/standby-runner/lib.sh}"

readonly VARIABLE=CI_RUNNER
readonly BILLING_MESSAGE='The job was not started because recent account payments have failed or your spending limit needs to be increased'
readonly STATE_FILE="${STANDBY_RUNNER_STATE_DIR}/handled"
readonly STATE_LINES=2000
readonly RUNS_PER_REPO=20

umask 077
GH_WORK="$(mktemp -d)"
trap 'rm -rf "${GH_WORK}"' EXIT
touch "${STATE_FILE}"

list_repos() {
  local page=1 names count
  while :; do
    names="$(gh_call GET "/installation/repositories?per_page=100&page=${page}" | jq -r '.repositories[].full_name')"
    [[ -z "${names}" ]] || printf '%s\n' "${names}"
    count="$(printf '%s' "${names}" | grep -c . || true)"
    ((count < 100)) || {
      page=$((page + 1))
      continue
    }
    return 0
  done
}

clear_variable() {
  local repo="$1"
  gh_api DELETE "/repos/${repo}/actions/variables/${VARIABLE}"
  gh_succeeded || [[ "${GH_STATUS}" == 404 ]] \
    || die "clearing ${VARIABLE} in ${repo} answered HTTP ${GH_STATUS}"
}

set_variable() {
  local repo="$1" body
  body="$(jq -n --arg name "${VARIABLE}" --arg value "${STANDBY_RUNNER_LABEL}" '{name: $name, value: $value}')"
  gh_api PATCH "/repos/${repo}/actions/variables/${VARIABLE}" "${body}"
  gh_succeeded && return 0
  [[ "${GH_STATUS}" == 404 ]] || die "setting ${VARIABLE} in ${repo} answered HTTP ${GH_STATUS}"
  gh_call POST "/repos/${repo}/actions/variables" "${body}" >/dev/null
}

# Prints "yes" or "no". Run in a command substitution so that an API error
# stops the script instead of reading as "no".
billing_verdict() {
  local repo="$1" run="$2" check_urls check_url annotations
  check_urls="$(gh_call GET "/repos/${repo}/actions/runs/${run}/jobs" \
    | jq -r '.jobs[] | select((.steps | length) == 0 and ((.runner_name // "") == "")) | .check_run_url')"
  while IFS= read -r check_url; do
    [[ -n "${check_url}" ]] || continue
    annotations="$(gh_call GET "/repos/${check_url#*/repos/}/annotations")"
    if jq -e --arg message "${BILLING_MESSAGE}" 'any(.[]; .message | contains($message))' <<<"${annotations}" >/dev/null; then
      echo yes
      return 0
    fi
  done <<<"${check_urls}"
  echo no
}

handle_run() {
  local repo="$1" run="$2" verdict
  verdict="$(billing_verdict "${repo}" "${run}")"
  if [[ "${verdict}" == yes ]]; then
    set_variable "${repo}"
    gh_call POST "/repos/${repo}/actions/runs/${run}/rerun-failed-jobs" >/dev/null
  fi
  printf '%s %s\n' "${repo}" "${run}" >>"${STATE_FILE}"
}

token="$(mint_installation_token)"
gh_use_token "${token}"
repos="$(list_repos)"

if [[ "$(date -u -d "@${STANDBY_RUNNER_NOW:-$(date +%s)}" +%d)" == 01 ]]; then
  while IFS= read -r repo; do
    [[ -n "${repo}" ]] && clear_variable "${repo}"
  done <<<"${repos}"
  exit 0
fi

while IFS= read -r repo; do
  [[ -n "${repo}" ]] || continue
  runs="$(gh_call GET "/repos/${repo}/actions/runs?status=failure&per_page=${RUNS_PER_REPO}" | jq -r '.workflow_runs[].id')"
  while IFS= read -r run; do
    [[ -n "${run}" ]] || continue
    grep -qxF "${repo} ${run}" "${STATE_FILE}" || handle_run "${repo}" "${run}"
  done <<<"${runs}"
done <<<"${repos}"

tail -n "${STATE_LINES}" "${STATE_FILE}" >"${STATE_FILE}.tmp"
mv "${STATE_FILE}.tmp" "${STATE_FILE}"
