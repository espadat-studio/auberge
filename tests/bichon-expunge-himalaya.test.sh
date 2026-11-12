#!/usr/bin/env bash
#
# tests/bichon-expunge-himalaya.test.sh
#
# #1313: every himalaya call examples/bichon-expunge.sh makes, checked against
# the real himalaya binary. tests/bichon-expunge.test.sh stubs himalaya, so it
# stayed green while the real CLI refused v1 flags (#1301) and a uid set that a
# variadic --flag swallowed (#1308). Both reached a live sweep.
#
# himalaya parses its arguments before it reads an account, so each call runs
# with an account the config does not hold. clap exits 2 on a parse error;
# "Account `<name>` not found" means the arguments parsed. The config is an
# empty [accounts] table, so no call can reach a mail server.
#
# The script's own functions issue the calls. The himalaya shim below checks
# each one against the binary, then answers with a canned reply so the function
# goes on to its next call.
#
# Run: ./tests/bichon-expunge-himalaya.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
SCRIPT="${REPO_ROOT}/examples/bichon-expunge.sh"
readonly SUITE_DIR REPO_ROOT SCRIPT

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"
# shellcheck source=../examples/bichon-expunge.sh disable=SC1091
source "${SCRIPT}"

HIMALAYA_BIN=$(type -P himalaya) || {
  printf 'himalaya is not on PATH; this suite needs the real binary\n' >&2
  exit 1
}
readonly HIMALAYA_BIN
printf 'against %s\n' "$("${HIMALAYA_BIN}" --version | head -n1)"

CONFIG_DIR=$(mktemp -d)
trap 'rm -rf -- "${CONFIG_DIR}"' EXIT
printf '[accounts]\n' >"${CONFIG_DIR}/config.toml"
export HIMALAYA_CONFIG="${CONFIG_DIR}/config.toml"

readonly UNKNOWN_ACCOUNT='contract-check@invalid'

# Exit 0 (a config-only command) or exit 1 naming the unknown account. Exit 2 is
# clap refusing the arguments; anything else is a failure this suite did not
# anticipate, and is reported rather than guessed at.
himalaya_accepts() {
  local out rc=0
  out=$("${HIMALAYA_BIN}" "$@" 2>&1 </dev/null) || rc=$?
  ((rc == 0)) && return 0
  ((rc == 1)) && [[ "${out}" == *"Account \`${UNKNOWN_ACCOUNT}\` not found"* ]] && return 0
  printf '       himalaya %s\n       exit %s: %s\n' "$*" "${rc}" "${out}" >&2
  return 1
}

# Most calls run inside $(...), so the shim's record goes to files: a variable
# set in that subshell is gone when it returns.
CALLS="${CONFIG_DIR}/calls" REFUSED="${CONFIG_DIR}/refused"
: >"${CALLS}"
: >"${REFUSED}"

# shellcheck disable=SC2329  # invoked by the sourced script's functions
himalaya() {
  printf '%s\n' "$*" >>"${CALLS}"
  himalaya_accepts "$@" 2>>"${REFUSED}" || printf '%s\n' "$*" >>"${REFUSED}"
  case "$*" in
    '--json account list') printf '%s' '{"accounts":[]}' ;;
    '--json imap list '*) printf '%s' '{"mailboxes":[{"name":"INBOX"}]}' ;;
    '--json imap search '*--before*) printf '%s' '{"ids":[{"id":7}],"uid_mode":true}' ;;
    '--json imap search '*--deleted*) printf '%s' '{"ids":[],"uid_mode":true}' ;;
    *) ;;
  esac
}

printf '== the checker refuses the shapes that reached a live sweep\n'

assert_fails 'v1 --output json is a parse error (#1301)' \
  himalaya_accepts imap list --all --account "${UNKNOWN_ACCOUNT}" --output json 2>/dev/null
assert_fails 'a uid set after --flag without -- is a parse error (#1308)' \
  himalaya_accepts imap store --account "${UNKNOWN_ACCOUNT}" --mailbox INBOX --flag '\Deleted' 7,9 2>/dev/null
assert_succeeds 'an unknown account past a clean parse is accepted' \
  himalaya_accepts imap expunge --account "${UNKNOWN_ACCOUNT}" INBOX

printf '\n== every call the script issues parses\n'

# check_tools probes auberge and ssh too; only its himalaya call is under test.
# shellcheck disable=SC2329  # found by check_tools' command -v
auberge() { :; }
# shellcheck disable=SC2329  # found by check_tools' command -v
ssh() { :; }
# shellcheck disable=SC2329  # overrides the Host walk probe_folder_coverage reaches last
ensure_account_sidecar_rows() { return 1; }

# shellcheck disable=SC2034  # read by the sourced script's functions
account="${UNKNOWN_ACCOUNT}" folder='INBOX' window_days=90

check_tools 2>/dev/null
fetch_account_folders
probe_folder_coverage 2>/dev/null
assert_eq 'the probe ran past both searches' 'err-sidecars' "${PAIR_STATUS}"
expunge_ids "${UNKNOWN_ACCOUNT}" INBOX 7 9 2>/dev/null

assert_eq 'one call per command shape' $'--json account list
--json imap list --all --account contract-check@invalid
--json imap search --account contract-check@invalid --mailbox INBOX --before '"${CUTOFF_DATE}"'
--json imap search --account contract-check@invalid --mailbox INBOX --deleted
imap store --account contract-check@invalid --mailbox INBOX --flag \Deleted -- 7,9
imap expunge --account contract-check@invalid INBOX' \
  "$(<"${CALLS}")"

assert_eq 'himalaya refused none of them' '' "$(<"${REFUSED}")"

# A call site this suite does not drive goes unchecked. Counting them makes a
# new one fail here until the driving above reaches it.
assert_eq 'the script has as many himalaya call sites as this suite drives' '6' \
  "$(grep -cE '\$\(himalaya_json (account|imap) |^[[:space:]]*himalaya (account|imap) .*(\\|return [0-9])$' "${SCRIPT}")"

report 'bichon-expunge-himalaya'
