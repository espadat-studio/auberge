#!/usr/bin/env bash
#
# tests/standby-runner-flip.test.sh
#
# Unit tests for the Runner Flip poller (ADR-0085): which failed runs flip
# CI_RUNNER and get a re-run, and which are left alone.
#
# No network, no GitHub: curl is a stub on PATH that answers from fixtures each
# case stages, and logs every request. Anything the case did not stage answers
# HTTP 599, so an unexpected call fails the run instead of passing quietly. The
# App key is a throwaway generated here, because the poller really signs a JWT.
#
# Run: ./tests/standby-runner-flip.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly ROLE_FILES="${REPO_ROOT}/ansible/roles/standby/files"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

readonly BIN="${WORK}/bin"
readonly FIXTURES="${WORK}/fixtures"
readonly REQUESTS="${WORK}/requests.log"
readonly KEY="${WORK}/app.pem"
readonly API=https://api.github.test
readonly BILLING='The job was not started because recent account payments have failed or your spending limit needs to be increased'

mkdir -p "${BIN}"
openssl genrsa -out "${KEY}" 2048 2>/dev/null

cat >"${BIN}/curl" <<'STUB'
#!/usr/bin/env bash
method=GET out=
while (($#)); do
  case "$1" in
    -X) method="$2"; shift ;;
    -o) out="$2"; shift ;;
    http*) url="$1" ;;
  esac
  shift
done
path="${url#"${STUB_API}"}"
printf '%s %s\n' "${method}" "${path}" >>"${STUB_REQUESTS}"
name="${method}${path%%\?*}"
name="${name//\//_}"
if [[ -f "${STUB_FIXTURES}/${name}.status" ]]; then
  cp "${STUB_FIXTURES}/${name}.body" "${out}"
  cat "${STUB_FIXTURES}/${name}.status"
else
  echo 'no fixture' >"${out}"
  echo 599
fi
STUB
chmod 0755 "${BIN}/curl"
export PATH="${BIN}:${PATH}"
export STUB_API="${API}" STUB_FIXTURES="${FIXTURES}" STUB_REQUESTS="${REQUESTS}"

# fixture METHOD PATH STATUS BODY
fixture() {
  local name="$1${2}"
  name="${name//\//_}"
  printf '%s' "$4" >"${FIXTURES}/${name}.body"
  printf '%s' "$3" >"${FIXTURES}/${name}.status"
}

new_case() {
  rm -rf "${FIXTURES}" "${REQUESTS}" "${WORK}/state"
  mkdir -p "${FIXTURES}" "${WORK}/state"
  : >"${REQUESTS}"
  fixture GET /app/installations 200 '[{"id":42}]'
  fixture POST /app/installations/42/access_tokens 201 '{"token":"tok"}'
  fixture GET /installation/repositories 200 '{"repositories":[{"full_name":"o/r"}]}'
}

# stage_run RUN_ID STEPS_JSON RUNNER_NAME_JSON ANNOTATION_MESSAGE
stage_run() {
  local run="$1" steps="$2" runner="$3" message="$4"
  fixture GET /repos/o/r/actions/runs 200 "{\"workflow_runs\":[{\"id\":${run}}]}"
  fixture GET "/repos/o/r/actions/runs/${run}/jobs" 200 \
    "{\"jobs\":[{\"steps\":${steps},\"runner_name\":${runner},\"check_run_url\":\"${API}/repos/o/r/check-runs/9\"}]}"
  fixture GET /repos/o/r/check-runs/9/annotations 200 "[{\"message\":\"${message}\"}]"
  fixture PATCH /repos/o/r/actions/variables/CI_RUNNER 200 '{}'
  fixture POST "/repos/o/r/actions/runs/${run}/rerun-failed-jobs" 201 '{}'
}

# poll [NOW_EPOCH] runs the poller and prints its exit status.
poll() {
  local rc=0
  STANDBY_RUNNER_API="${API}" STANDBY_RUNNER_APP_ID=1 STANDBY_RUNNER_KEY_PATH="${KEY}" \
    STANDBY_RUNNER_LABEL=ci-selfhosted STANDBY_RUNNER_STATE_DIR="${WORK}/state" \
    STANDBY_RUNNER_LIB="${ROLE_FILES}/standby-runner-lib.sh" \
    STANDBY_RUNNER_NOW="${1:-$(date -u -d 2026-10-22 +%s)}" \
    "${ROLE_FILES}/standby-runner-flip.sh" >/dev/null 2>&1 || rc=$?
  printf '%s' "${rc}"
}

count_requests() {
  grep -c -x -F "$1" "${REQUESTS}" || true
}

count_mutations() {
  grep -c -E '^(POST|PATCH|DELETE) /repos/' "${REQUESTS}" || true
}

new_case
stage_run 1 '[]' null "${BILLING}"
assert_eq "billing failure: poller succeeds" 0 "$(poll)"
assert_eq "billing failure: CI_RUNNER set" 1 "$(count_requests 'PATCH /repos/o/r/actions/variables/CI_RUNNER')"
assert_eq "billing failure: failed jobs re-run once" 1 "$(count_requests 'POST /repos/o/r/actions/runs/1/rerun-failed-jobs')"

assert_eq "same run seen twice: poller succeeds" 0 "$(poll)"
assert_eq "same run seen twice: still one re-run" 1 "$(count_requests 'POST /repos/o/r/actions/runs/1/rerun-failed-jobs')"
assert_eq "same run seen twice: CI_RUNNER set once" 1 "$(count_requests 'PATCH /repos/o/r/actions/variables/CI_RUNNER')"

new_case
stage_run 2 '[{"name":"cargo test"}]' '"hosted-7"' ''
assert_eq "ordinary failure: poller succeeds" 0 "$(poll)"
assert_eq "ordinary failure: nothing mutated" 0 "$(count_mutations)"

new_case
stage_run 3 '[]' '""' 'Process completed with exit code 1.'
assert_eq "zero steps without the billing annotation: poller succeeds" 0 "$(poll)"
assert_eq "zero steps without the billing annotation: nothing mutated" 0 "$(count_mutations)"

new_case
stage_run 4 '[]' null "${BILLING}"
fixture PATCH /repos/o/r/actions/variables/CI_RUNNER 404 '{}'
fixture POST /repos/o/r/actions/variables 201 '{}'
assert_eq "variable absent: poller succeeds" 0 "$(poll)"
assert_eq "variable absent: CI_RUNNER created" 1 "$(count_requests 'POST /repos/o/r/actions/variables')"
assert_eq "variable absent: failed jobs re-run" 1 "$(count_requests 'POST /repos/o/r/actions/runs/4/rerun-failed-jobs')"

new_case
stage_run 5 '[]' null "${BILLING}"
fixture DELETE /repos/o/r/actions/variables/CI_RUNNER 204 ''
assert_eq "UTC day 1: poller succeeds" 0 "$(poll "$(date -u -d 2026-11-01T00:10:00 +%s)")"
assert_eq "UTC day 1: CI_RUNNER cleared" 1 "$(count_requests 'DELETE /repos/o/r/actions/variables/CI_RUNNER')"
assert_eq "UTC day 1: nothing re-run" 0 "$(count_requests 'POST /repos/o/r/actions/runs/5/rerun-failed-jobs')"

new_case
fixture DELETE /repos/o/r/actions/variables/CI_RUNNER 404 '{}'
assert_eq "UTC day 1, variable already absent: poller succeeds" 0 "$(poll "$(date -u -d 2026-11-01T00:10:00 +%s)")"

new_case
fixture GET /repos/o/r/actions/runs 500 '{}'
assert_fails "API error is not read as no failures" test "$(poll)" = 0

report "standby-runner-flip"
