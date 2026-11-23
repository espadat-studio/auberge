#!/usr/bin/env bash
#
# tests/knowledge-vault-grant.test.sh
#
# Unit tests for the hermes role's Knowledge Vault grant (#1334): an App reads
# the whole Vault and writes only the subtrees config names, and a subtree
# dropped from config goes back to read-only.
#
# No root: the grantee is `nobody` and the operator is the user running the
# test, so every setfacl lands on files that user owns. The probe runs as that
# user too, which is enough to pin its verdict on what access(2) reports.
#
# Run: ./tests/knowledge-vault-grant.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT
readonly SCRIPT="${REPO_ROOT}/ansible/roles/hermes/files/knowledge-vault-grant.sh"

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"

WORK="$(mktemp -d)"
trap 'chmod -R u+w "${WORK}"; rm -rf "${WORK}"' EXIT

readonly VAULT="${WORK}/home/vault"
readonly OUTSIDE="${WORK}/outside"
OPERATOR="$(id -un)"
readonly OPERATOR

reset_vault() {
  chmod -R u+w "${WORK}"
  rm -rf "${WORK:?}"/*
  mkdir -p "${VAULT}/journal/2026" "${VAULT}/private" "${OUTSIDE}"
  chmod 700 "${WORK}/home"
  touch "${VAULT}/note.md" "${VAULT}/journal/2026/day.md" "${VAULT}/private/secret.md"
  ln -s "${OUTSIDE}" "${VAULT}/escape"
}

grant() { "${SCRIPT}" grant "${VAULT}" "${OPERATOR}" nobody "$@"; }
probe() { "${SCRIPT}" probe "${VAULT}" "$@" >/dev/null 2>&1; }
nobody_acl() { getfacl -cp "$@" 2>/dev/null | grep -E '^(default:)?user:nobody:' | tr '\n' ' '; }

reset_vault
assert_fails 'an absolute subtree is refused' grant /etc
assert_fails 'a subtree with .. is refused' grant journal/../../x
assert_fails 'a subtree through a symlink out of the vault is refused' grant escape/x
assert_eq 'a refused grant changes nothing' '' "$(nobody_acl "${VAULT}" "${WORK}/home")"

reset_vault
grant journal >/dev/null
assert_eq 'the vault root is read-only' \
  'user:nobody:r-x default:user:nobody:r-x ' "$(nobody_acl "${VAULT}")"
assert_eq 'a file outside the subtree is read-only' 'user:nobody:r-- ' "$(nobody_acl "${VAULT}/note.md")"
assert_eq 'a subtree directory is writable' \
  'user:nobody:rwx default:user:nobody:rwx ' "$(nobody_acl "${VAULT}/journal/2026")"
assert_eq 'a subtree file is writable' 'user:nobody:rw- ' "$(nobody_acl "${VAULT}/journal/2026/day.md")"
assert_eq 'an untraversable ancestor gets traverse only' 'user:nobody:--x ' "$(nobody_acl "${WORK}/home")"
assert_eq 'a symlinked directory is not followed' '' "$(nobody_acl "${OUTSIDE}")"
assert_eq 'a converged grant reports nothing' '' "$(grant journal)"

grant >/dev/null
assert_eq 'a dropped subtree is revoked to read-only' \
  'user:nobody:r-x default:user:nobody:r-x ' "$(nobody_acl "${VAULT}/journal")"

reset_vault
grant inbox/agent >/dev/null
assert_succeeds 'a missing subtree is created' test -d "${VAULT}/inbox/agent"
assert_eq 'a created subtree is owned by the operator' "${OPERATOR}" "$(stat -c %U "${VAULT}/inbox/agent")"

reset_vault
grant journal >/dev/null
"${SCRIPT}" revoke "${VAULT}" "${OPERATOR}" nobody >/dev/null
assert_eq 'a revoked grant leaves no entry in the vault' '' \
  "$(nobody_acl "${VAULT}" "${VAULT}/note.md" "${VAULT}/journal" "${VAULT}/journal/2026/day.md")"
assert_eq 'a revoked grant leaves no traverse entry on an ancestor' '' "$(nobody_acl "${WORK}/home")"
assert_eq 'a converged revoke reports nothing' '' "$("${SCRIPT}" revoke "${VAULT}" "${OPERATOR}" nobody)"

reset_vault
handed="$("${SCRIPT}" grant "${VAULT}" "${OPERATOR}" "${OPERATOR}" journal | grep '^chown ' || true)"
assert_succeeds 'a grantee file outside the subtrees goes to the operator' \
  grep -qx "chown ${VAULT}/note.md" <<<"${handed}"
assert_fails 'a grantee file inside a granted subtree stays the grantee'"'"'s' \
  grep -q "^chown ${VAULT}/journal" <<<"${handed}"

reset_vault
chmod -R a-w "${VAULT}"
chmod u+w "${VAULT}/journal"
assert_succeeds 'the probe passes when only the subtrees are writable' probe journal
assert_fails 'the probe fails when a declared subtree is not writable' probe private
chmod u+w "${VAULT}/private/secret.md"
assert_fails 'the probe fails when anything outside the subtrees is writable' probe journal

report 'knowledge-vault-grant'
