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
#   3. oauth app    one redirect URI per <site-origin>, each the Decap admin
#                   page itself, and confidential_client is false. A browser
#                   app holds no secret, and leaving it true is the documented
#                   cause of "Impossible to login with forgejo"
#                   (decap-cms#7867).
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
# Re-running is safe, and it is also how the redirect URIs are changed. The
# migrate and mirror steps look for their own result and skip it; the OAuth
# step reconciles instead, PATCHing the application to exactly the origins
# given on this run. The client ID survives that, so the site's Decap config
# never has to be edited — a site moving from a staging host to its real
# domain is one re-run with both origins passed.
#
# Pass every origin the admin page will be served from. OAuth redirect
# matching is exact: an origin that is not listed cannot log in, and a
# per-deployment preview URL will not match the production one.
#
# Verified against Forgejo 16.0.5+gitea-1.22.0.

set -euo pipefail

readonly USAGE="usage: ${0##*/} <github-owner/repo> <site-origin>...

  ${0##*/} espadat-studio/nathalieconan.fr https://nathalieconan.fr \\
    https://site.example.workers.dev http://localhost:4321

Each <site-origin> becomes the redirect URI <site-origin>/admin/. The set is
replaced on every run, so pass all of them every time."

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
  [[ $# -ge 2 ]] || die "$USAGE"

  local source="$1"
  shift
  [[ $source == */* ]] || die "expected <github-owner/repo>, got: ${source}"

  local var
  for var in FORGEJO_URL FORGEJO_TOKEN GITHUB_TOKEN; do
    [[ -n ${!var:-} ]] || die "${var} is not set"
  done

  command -v jq >/dev/null || die 'jq is required'

  forge="${FORGEJO_URL%/}"
  local repo="${source#*/}"
  local clone="https://github.com/${source}.git"

  local site redirects=()
  for site in "$@"; do
    [[ $site == http*://* ]] || die "expected an origin URL, got: ${site}"
    redirects+=("${site%/}/admin/")
  done

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

  local app_name="decap-${repo}"
  step "OAuth application ${app_name}"
  local apps existing payload client_id
  apps="$(api GET /user/applications/oauth2)"
  existing="$(jq -c --arg n "${app_name}" \
    'map(select(.name == $n)) | first // empty' <<<"${apps}")"
  payload="$(jq -n --arg name "${app_name}" \
    --argjson uris "$(jq -nc '$ARGS.positional' --args "${redirects[@]}")" \
    '{name: $name, redirect_uris: $uris, confidential_client: false}')"
  if [[ -n ${existing} ]]; then
    # PATCH rather than skip: the redirect URIs are the one thing a re-run is
    # expected to change, and the client ID survives it.
    client_id="$(jq -r '.client_id' <<<"${existing}")"
    api PATCH "/user/applications/oauth2/$(jq -r '.id' <<<"${existing}")" \
      --data "${payload}" >/dev/null
    note 'already registered, client ID unchanged'
  else
    client_id="$(api POST /user/applications/oauth2 --data "${payload}" \
      | jq -r '.client_id')"
    note 'registered as a public client, no secret to store'
  fi
  for site in "${redirects[@]}"; do
    note "redirect ${site}"
  done

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
