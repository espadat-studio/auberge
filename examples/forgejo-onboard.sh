#!/usr/bin/env bash
#
# examples/forgejo-onboard.sh
#
# REFERENCE SCRIPT — NOT SHIPPED IN THE auberge BINARY
# ---------------------------------------------------
# Onboards one client site onto a self-hosted Forgejo so that an editor who has
# no github.com account can edit its copy through Decap CMS. Two things to
# create, in order:
#
#   1. repository   creates a private content repository on the forge, holding
#                   only the README that initialised it. Decap commits the
#                   editable copy into it, and nothing else writes there.
#   2. oauth app    one redirect URI per <site-origin>, each the Decap admin
#                   page itself, and confidential_client is false. A browser
#                   app holds no secret, and leaving it true is the documented
#                   cause of "Impossible to login with forgejo"
#                   (decap-cms#7867).
#
# It then prints the backend block to paste into the site's Decap config.
#
# GITHUB STAYS ORIGIN, AND THIS SCRIPT DOES NOT TOUCH IT. The content
# repository is not a copy of the site: it shares no history with it and has no
# git relation to github.com. The site's own CI carries an edit from the forge
# into its source and deploys from there.
#
# Do not migrate the site's repository onto the forge, and do not add a push
# mirror back to GitHub. A push mirror force-pushes with --mirror, which
# deletes every branch that exists only on GitHub and closes its pull request
# — Renovate and template-sync branches are exactly that. The per-branch
# filter does not prevent it. Recorded in full under "Why not migrate the site
# and mirror back" in the forgejo role README.
#
# Configuration is the environment, and the script knows nothing about any
# secret store:
#
#   FORGEJO_URL     base URL of the forge, e.g. https://git.example.com
#   FORGEJO_TOKEN   Forgejo token, scopes write:repository and write:user
#
# Re-running is safe, and it is also how the redirect URIs are changed. The
# repository step looks for its own result and skips it; the OAuth step
# reconciles instead, PATCHing the application to exactly the origins given on
# this run. The client ID survives that, so the site's Decap config never has
# to be edited — a site moving from a staging host to its real domain is one
# re-run with both origins passed.
#
# Pass every origin the admin page will be served from. OAuth redirect
# matching is exact: an origin that is not listed cannot log in, and a
# per-deployment preview URL will not match the production one.
#
# Verified against Forgejo 16.0.5+gitea-1.22.0.

set -euo pipefail

readonly USAGE="usage: ${0##*/} <content-repo> <site-origin>...

  ${0##*/} client-content https://client.example.com \\
    http://localhost:4321

<content-repo> is a bare name, created under the account FORGEJO_TOKEN belongs
to. Each <site-origin> becomes the redirect URI <site-origin>/admin/. The set
is replaced on every run, so pass all of them every time."

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

# <site-origin> -> the redirect URI to register, or non-zero if the argument is
# not an origin. Decap's redirect target is the admin page itself, trailing
# slash included: /admin/ and /admin are two different URIs to an exact
# matcher, and only one of them is what the site serves.
redirect_uri_for() {
  local site="$1"
  [[ $site == http://?* || $site == https://?* ]] || return 1
  printf '%s/admin/' "${site%/}"
}

# A bare repository name. Anchored on an alphanumeric because curl resolves a
# `.` or `..` segment out of a request path: a repository named `..` would
# probe /repos/ and the answer would be read as this repository's.
repo_name_is_bare() {
  [[ $1 =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]
}

api() {
  local method="$1" path="$2"
  shift 2
  curl -fsS --max-time 60 --config "${curlrc}" \
    -X "${method}" -H 'Content-Type: application/json' \
    "$@" "${forge}/api/v1${path}"
}

api_code() {
  curl -sS -o /dev/null -w '%{http_code}' --max-time 60 \
    --config "${curlrc}" "${forge}/api/v1$1"
}

main() {
  [[ $# -ge 2 ]] || die "$USAGE"

  local repo="$1"
  shift
  # So that the old <github-owner/repo> argument is refused rather than turned
  # into a repository with a slash in its name. Nothing here is derived from
  # GitHub any more.
  repo_name_is_bare "${repo}" \
    || die "expected a bare repository name, got: ${repo}"

  local var
  for var in FORGEJO_URL FORGEJO_TOKEN; do
    [[ -n ${!var:-} ]] || die "${var} is not set"
  done

  command -v jq >/dev/null || die 'jq is required'

  forge="${FORGEJO_URL%/}"

  local site redirects=()
  for site in "$@"; do
    redirects+=("$(redirect_uri_for "${site}")") \
      || die "expected an origin URL, got: ${site}"
  done

  # The token is written to a 0600 config rather than passed as an argument, so
  # it never appears in the process list.
  curlrc="$(mktemp)"
  trap 'rm -f "${curlrc}"' EXIT
  chmod 600 "${curlrc}"
  printf 'header = "Authorization: token %s"\n' "${FORGEJO_TOKEN}" >"${curlrc}"

  local owner
  owner="$(api GET /user | jq -er '.login')" \
    || die "cannot reach ${forge} — check FORGEJO_URL and FORGEJO_TOKEN"

  # The branch the backend block names is read off the forge, never assumed. A
  # repository created by hand in the web UI lands on whatever that forge
  # defaults to, and naming the wrong branch is the silent load failure the
  # role README warns about.
  local code branch
  step "Content repository ${owner}/${repo}"
  code="$(api_code "/repos/${owner}/${repo}")"
  case "${code}" in
    200)
      branch="$(api GET "/repos/${owner}/${repo}" \
        | jq -er '.default_branch')" \
        || die "cannot read the default branch of ${owner}/${repo}"
      note 'already on the forge, leaving it alone'
      ;;
    404)
      # auto_init, because Decap resolves `branch` on load and an
      # uninitialised repository has no branch to resolve — the CMS fails
      # before the editor sees a field. So it is empty of content, not of
      # commits.
      branch='master'
      jq -n --arg name "${repo}" --arg branch "${branch}" \
        '{name: $name, default_branch: $branch, private: true,
          auto_init: true, readme: "Default"}' \
        | api POST /user/repos --data @- >/dev/null
      note "created private and initialised on ${branch}"
      ;;
    *)
      die "unexpected HTTP ${code} from ${forge} for ${owner}/${repo}"
      ;;
  esac

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
  local uri
  for uri in "${redirects[@]}"; do
    note "redirect ${uri}"
  done

  step 'Decap backend block'
  cat <<CONFIG

backend:
  name: gitea
  base_url: ${forge}
  api_root: ${forge}/api/v1
  repo: ${owner}/${repo}
  branch: ${branch}
  app_id: ${client_id}

CONFIG
  note 'Remaining by hand: give the editor a Forgejo account and write access'
  note 'to this repository, and wire the site CI that carries a commit here'
  note 'into the site source on GitHub. GitHub stays origin; nothing mirrors.'
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
