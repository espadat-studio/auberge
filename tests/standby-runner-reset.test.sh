#!/usr/bin/env bash
#
# tests/standby-runner-reset.test.sh
#
# Unit tests for the Standby Runner's per-job reset (ADR-0085): every job starts
# from the root-owned release and an empty HOME, and the release only moves to
# a download whose digest matches what GitHub published for it.
#
# No network, no root: curl is a stub on PATH that answers API calls from
# fixtures and downloads from staged files, and logs every request. Anything
# not staged fails, so an unexpected call fails the run. The runner user is the
# one running the test, since only root can chown to anyone else.
#
# Run: ./tests/standby-runner-reset.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly ROLE_FILES="${REPO_ROOT}/ansible/roles/standby/files"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'chmod -R u+w "${WORK}"; rm -rf "${WORK}"' EXIT

readonly BIN="${WORK}/bin"
readonly FIXTURES="${WORK}/fixtures"
readonly DOWNLOADS="${WORK}/downloads"
readonly REQUESTS="${WORK}/requests.log"
readonly KEY="${WORK}/app.pem"
readonly API=https://api.github.test
readonly RELEASE="${WORK}/opt/release"
readonly JOB="${WORK}/opt/job"
readonly RUNNER_HOME="${WORK}/home"

mkdir -p "${BIN}" "${WORK}/opt"
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
if [[ "${url}" != "${STUB_API}"* ]]; then
  printf 'DOWNLOAD %s\n' "${url}" >>"${STUB_REQUESTS}"
  [[ -f "${STUB_DOWNLOADS}/${url##*/}" ]] || exit 22
  cp "${STUB_DOWNLOADS}/${url##*/}" "${out}"
  exit 0
fi
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
export STUB_API="${API}" STUB_FIXTURES="${FIXTURES}" STUB_DOWNLOADS="${DOWNLOADS}" STUB_REQUESTS="${REQUESTS}"

# fixture METHOD PATH STATUS BODY
fixture() {
  local name="$1${2}"
  name="${name//\//_}"
  printf '%s' "$4" >"${FIXTURES}/${name}.body"
  printf '%s' "$3" >"${FIXTURES}/${name}.status"
}

# stage_release VERSION [DIGEST] builds a runner archive for VERSION and
# publishes it as the latest release, with its real digest unless one is given.
stage_release() {
  local version="$1" tree="${WORK}/tree-$1" name="actions-runner-linux-x64-$1.tar.gz" digest
  mkdir -p "${tree}/bin"
  printf '#!/bin/bash\necho %s\n' "${version}" >"${tree}/run.sh"
  printf '#!/bin/bash\n' >"${tree}/run-helper.sh.template"
  printf 'listener %s\n' "${version}" >"${tree}/bin/Runner.Listener"
  chmod 0777 "${tree}/run.sh" "${tree}/bin/Runner.Listener"
  tar -czf "${DOWNLOADS}/${name}" -C "${tree}" .
  digest="${2:-$(sha256sum "${DOWNLOADS}/${name}" | cut -d' ' -f1)}"
  fixture GET /repos/actions/runner/releases/latest 200 "$(jq -n \
    --arg tag "v${version}" --arg name "${name}" --arg digest "sha256:${digest}" \
    --arg url "https://github.test/actions/runner/releases/download/v${version}/${name}" \
    '{tag_name: $tag, assets: [{name: "actions-runner-osx-arm64.tar.gz"}, {name: $name, digest: $digest, browser_download_url: $url}]}')"
}

new_case() {
  rm -rf "${FIXTURES}" "${DOWNLOADS}" "${REQUESTS}"
  mkdir -p "${FIXTURES}" "${DOWNLOADS}" "${RUNNER_HOME}"
  : >"${REQUESTS}"
  fixture GET /app/installations 200 '[{"id":42}]'
  fixture POST /app/installations/42/access_tokens 201 '{"token":"tok"}'
}

# reset runs the reset and prints its exit status.
reset() {
  local rc=0
  STANDBY_RUNNER_API="${API}" STANDBY_RUNNER_APP_ID=1 STANDBY_RUNNER_KEY_PATH="${KEY}" \
    STANDBY_RUNNER_LIB="${ROLE_FILES}/standby-runner-lib.sh" \
    STANDBY_RUNNER_RELEASE_DIR="${RELEASE}" STANDBY_RUNNER_JOB_DIR="${JOB}" \
    STANDBY_RUNNER_HOME="${RUNNER_HOME}" \
    STANDBY_RUNNER_USER="$(id -un)" STANDBY_RUNNER_GROUP="$(id -gn)" \
    "${ROLE_FILES}/standby-runner-reset.sh" >/dev/null 2>&1 || rc=$?
  printf '%s' "${rc}"
}

downloads() {
  grep -c '^DOWNLOAD ' "${REQUESTS}" || true
}

inode() {
  stat -c %i "$1"
}

new_case
stage_release 2.338.1
mkdir "${JOB}"
assert_eq "fresh box: reset succeeds" 0 "$(reset)"
assert_eq "fresh box: release installed at the latest version" 2.338.1 "$(cat "${RELEASE}/.version")"
assert_eq "fresh box: archive modes lose group and other write" 755 "$(stat -c %a "${RELEASE}/run.sh")"
assert_eq "fresh box: job runs the release's run.sh" "$(inode "${RELEASE}/run.sh")" "$(inode "${JOB}/run.sh")"
assert_eq "fresh box: job binaries are the release's" \
  "$(inode "${RELEASE}/bin/Runner.Listener")" "$(inode "${JOB}/bin/Runner.Listener")"
assert_eq "fresh box: job root closed to other users" 750 "$(stat -c %a "${JOB}")"

new_case
stage_release 2.338.1
mkdir -p "${JOB}/_work/repo-a/src" "${RUNNER_HOME}/.cargo/bin" "${WORK}/outside"
echo private >"${JOB}/_work/repo-a/src/main.rs"
echo planted >"${JOB}/run-helper.sh"
echo shim >"${RUNNER_HOME}/.cargo/bin/cargo"
echo rc >"${RUNNER_HOME}/.bashrc"
echo kept >"${WORK}/outside/target"
ln -s "${WORK}/outside" "${RUNNER_HOME}/link"
chmod 0755 "${RUNNER_HOME}"
assert_eq "previous job left state: reset succeeds" 0 "$(reset)"
assert_eq "previous job left state: up-to-date release not downloaded again" 0 "$(downloads)"
assert_fails "previous job left state: other repo's checkout gone" test -e "${JOB}/_work"
assert_fails "previous job left state: planted file in job root gone" test -e "${JOB}/run-helper.sh"
assert_eq "previous job left state: HOME emptied" "" "$(ls -A "${RUNNER_HOME}")"
assert_eq "previous job left state: HOME closed again" 700 "$(stat -c %a "${RUNNER_HOME}")"
assert_eq "previous job left state: symlink target outside HOME untouched" kept "$(cat "${WORK}/outside/target")"

# The reset refills the job dir in place and never removes it. A parent without
# write permission makes removal fail, without root, as a mount point would.
new_case
stage_release 2.338.1
job_inode="$(inode "${JOB}")"
mkdir -p "${JOB}/_work"
echo private >"${JOB}/_work/leftover"
chmod 0555 "${WORK}/opt"
assert_eq "unremovable job dir: reset succeeds" 0 "$(reset)"
chmod 0755 "${WORK}/opt"
assert_eq "unremovable job dir: same directory rebuilt in place" "${job_inode}" "$(inode "${JOB}")"
assert_fails "unremovable job dir: previous job's files gone" test -e "${JOB}/_work"
assert_eq "unremovable job dir: job runs the release's run.sh" "$(inode "${RELEASE}/run.sh")" "$(inode "${JOB}/run.sh")"
assert_eq "unremovable job dir: release not nested inside" "" "$(ls -d "${JOB}/release" 2>/dev/null)"

new_case
stage_release 2.339.0
assert_eq "new release: reset succeeds" 0 "$(reset)"
assert_eq "new release: release moved to it" 2.339.0 "$(cat "${RELEASE}/.version")"
assert_eq "new release: job runs it" "echo 2.339.0" "$(tail -n1 "${JOB}/run.sh")"

new_case
stage_release 2.340.0 "$(printf '0%.0s' {1..64})"
assert_fails "digest mismatch: reset fails" test "$(reset)" = 0
assert_eq "digest mismatch: release left as it was" 2.339.0 "$(cat "${RELEASE}/.version")"

new_case
stage_release 2.340.0
fixture GET /repos/actions/runner/releases/latest 200 \
  "$(jq '.tag_name = "v2.340.0/../x"' "${FIXTURES}/GET_repos_actions_runner_releases_latest.body")"
assert_fails "malformed tag: reset fails" test "$(reset)" = 0
assert_eq "malformed tag: nothing downloaded" 0 "$(downloads)"

new_case
stage_release 2.340.0
fixture GET /repos/actions/runner/releases/latest 200 \
  "$(jq 'del(.assets[1].digest)' "${FIXTURES}/GET_repos_actions_runner_releases_latest.body")"
assert_fails "release without a digest: reset fails" test "$(reset)" = 0
assert_eq "release without a digest: release left as it was" 2.339.0 "$(cat "${RELEASE}/.version")"

new_case
fixture GET /repos/actions/runner/releases/latest 500 '{}'
assert_fails "API error: reset fails" test "$(reset)" = 0

report "standby-runner-reset"
