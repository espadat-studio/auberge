#!/usr/bin/env bash
#
# examples/forgejo-onboard.sh
#
# REFERENCE SCRIPT — NOT SHIPPED IN THE auberge BINARY
# ---------------------------------------------------
# Onboards one GitHub repository onto a self-hosted Forgejo so that an editor
# who has no github.com account can edit it through Decap CMS. Three API calls,
# in order:
#
#   1. migrate      clones the GitHub repository into Forgejo with its history.
#                   The forge becomes origin.
#   2. push mirror  points back at GitHub, syncing on every commit. GitHub goes
#                   on running CI and the deploy; it is now a replica.
#   3. oauth app    its redirect URI is the Decap admin page itself, and
#                   confidential_client is false. A browser app holds no secret,
#                   and leaving it true is the documented cause of "Impossible
#                   to login with forgejo" (decap-cms#7867).
#
# It then prints the backend block to paste into the site's Decap config.
#
# A push mirror FORCE-PUSHES. From step 2 on, the GitHub repository is a
# replica: anything pushed to it by hand is destroyed on the next sync, with no
# merge and no warning. Repoint your local origin at the forge. See the
# Push-mirroring section of the forgejo role README.
#
# Configuration is the environment, and the script knows nothing about any
# secret store:
#
#   FORGEJO_URL     base URL of the forge, e.g. https://git.example.com
#   FORGEJO_TOKEN   Forgejo token, scopes write:repository and write:user
#   GITHUB_TOKEN    GitHub PAT that can read the source repository, and push to
#                   it — the mirror reuses this one credential
#
# Re-running is safe. Every step looks for its own result first and skips it,
# so an interrupted run resumes and an onboarded repository reports its client
# ID again instead of registering a second application.
#
# Verified against Forgejo 16.0.5+gitea-1.22.0.

set -euo pipefail

readonly USAGE="usage: ${0##*/} <github-owner/repo> <site-origin>

  ${0##*/} espadat-studio/nathalieconan.fr https://nathalieconan.fr"

# Set by main() before the first api call. Globals rather than main()'s locals
# because the EXIT trap outlives main(): it runs once the frame is gone, so a
# trap over a local would silently rm nothing and strand the token on disk.
forge=''
curlrc=''

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

# Progress goes to stderr so that stdout carries only the Decap backend block
# and `forgejo-onboard.sh … > backend.yml` yields a pastable file.
step() {
  printf '\n\033[1m==>\033[0m %s\n' "$*" >&2
}

note() {
  printf '    %s\n' "$*" >&2
}

api() {
  local method="$1" path="$2"
  shift 2
  curl -fsS --max-time 120 --config "${curlrc}" \
    -X "${method}" -H 'Content-Type: application/json' \
    "$@" "${forge}/api/v1${path}"
}

api_code() {
  curl -sS -o /dev/null -w '%{http_code}' --max-time 60 \
    --config "${curlrc}" "${forge}/api/v1$1"
}

main() {
  [[ $# -eq 2 ]] || die "$USAGE"

  local source="$1" site="${2%/}"
  [[ $source == */* ]] || die "expected <github-owner/repo>, got: ${source}"

  local var
  for var in FORGEJO_URL FORGEJO_TOKEN GITHUB_TOKEN; do
    [[ -n ${!var:-} ]] || die "${var} is not set"
  done

  command -v jq >/dev/null || die 'jq is required'

  forge="${FORGEJO_URL%/}"
  local repo="${source#*/}"
  local redirect="${site}/admin/"
  local clone="https://github.com/${source}.git"

  # The token is written to a 0600 config rather than passed as an argument, so
  # it never appears in the process list.
  curlrc="$(mktemp)"
  trap 'rm -f "${curlrc}"' EXIT
  chmod 600 "${curlrc}"
  printf 'header = "Authorization: token %s"\n' "${FORGEJO_TOKEN}" >"${curlrc}"

  local owner
  owner="$(api GET /user | jq -r '.login')" \
    || die "cannot reach ${forge} — check FORGEJO_URL and FORGEJO_TOKEN"

  step "Repository ${owner}/${repo}"
  if [[ "$(api_code "/repos/${owner}/${repo}")" == 200 ]]; then
    note 'already on the forge, leaving it alone'
  else
    jq -n \
      --arg addr "${clone}" \
      --arg token "${GITHUB_TOKEN}" \
      --arg name "${repo}" \
      '{clone_addr: $addr, auth_token: $token, repo_name: $name,
        service: "github", private: true, mirror: false,
        issues: false, pull_requests: false, releases: false, wiki: false}' \
      | api POST /repos/migrate --data @- >/dev/null
    note "migrated from github.com/${source} with its history"
  fi

  step 'Push mirror back to GitHub'
  local mirrors
  mirrors="$(api GET "/repos/${owner}/${repo}/push_mirrors")"
  if [[ "$(jq -r --arg s "${source}" \
    '[.[] | select(.remote_address | contains($s))] | length' \
    <<<"${mirrors}")" != 0 ]]; then
    note 'already configured, leaving it alone'
  else
    jq -n \
      --arg addr "${clone}" \
      --arg user "${source%%/*}" \
      --arg token "${GITHUB_TOKEN}" \
      '{remote_address: $addr, remote_username: $user, remote_password: $token,
        sync_on_commit: true, interval: "8h0m0s"}' \
      | api POST "/repos/${owner}/${repo}/push_mirrors" --data @- >/dev/null
    note "syncing to github.com/${source} on every commit"
    note 'github.com is now a REPLICA — do not push to it by hand'
  fi

  step "OAuth application for ${redirect}"
  local app_name="decap-${repo}" apps client_id
  apps="$(api GET /user/applications/oauth2)"
  client_id="$(jq -r --arg n "${app_name}" \
    'map(select(.name == $n)) | first | .client_id // empty' <<<"${apps}")"
  if [[ -n ${client_id} ]]; then
    note 'already registered, reusing its client ID'
  else
    client_id="$(jq -n \
      --arg name "${app_name}" \
      --arg uri "${redirect}" \
      '{name: $name, redirect_uris: [$uri], confidential_client: false}' \
      | api POST /user/applications/oauth2 --data @- | jq -r '.client_id')"
    note 'registered as a public client, no secret to store'
  fi

  step 'Decap backend block'
  cat <<CONFIG

backend:
  name: gitea
  base_url: ${forge}
  api_root: ${forge}/api/v1
  repo: ${owner}/${repo}
  branch: master
  app_id: ${client_id}

CONFIG
  note 'Remaining by hand: give the editor a Forgejo account and write access,'
  note "then repoint origin: git remote set-url origin \
${forge}/${owner}/${repo}.git"
}

main "$@"
