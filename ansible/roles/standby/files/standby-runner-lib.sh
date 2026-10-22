#!/usr/bin/env bash
#
# Sourced, never run. GitHub App authentication and a status-aware API call for
# the Standby Runner's registration and Runner Flip scripts (ADR-0085).
#
# Needs from the caller: STANDBY_RUNNER_APP_ID, STANDBY_RUNNER_KEY_PATH, and a
# private directory in GH_WORK. Tokens go to a header file in GH_WORK and are
# passed with `curl -H @file`, because a bearer token on the command line is
# readable by every user through /proc.
#
# shellcheck shell=bash

shopt -s inherit_errexit

readonly STANDBY_RUNNER_API="${STANDBY_RUNNER_API:-https://api.github.com}"

die() {
  printf '%s: %s\n' "$(basename -- "$0")" "$*" >&2
  exit 1
}

b64url() {
  openssl base64 -A | tr '+/' '-_' | tr -d '='
}

mint_app_jwt() {
  local now header payload unsigned signature
  now="$(date +%s)"
  header="$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)"
  payload="$(printf '{"iat":%d,"exp":%d,"iss":"%s"}' \
    $((now - 60)) $((now + 540)) "${STANDBY_RUNNER_APP_ID}" | b64url)"
  unsigned="${header}.${payload}"
  signature="$(printf '%s' "${unsigned}" \
    | openssl dgst -sha256 -sign "${STANDBY_RUNNER_KEY_PATH}" -binary | b64url)"
  printf '%s.%s' "${unsigned}" "${signature}"
}

gh_use_token() {
  printf 'Authorization: Bearer %s\n' "$1" >"${GH_WORK}/auth"
}

# Sets GH_STATUS; the response body is left in ${GH_WORK}/body.
gh_api() {
  local method="$1" path="$2" data="${3-}"
  local args=(
    -sS -o "${GH_WORK}/body" -w '%{http_code}' -X "${method}"
    -H "@${GH_WORK}/auth"
    -H 'Accept: application/vnd.github+json'
    -H 'X-GitHub-Api-Version: 2022-11-28'
  )
  if [[ -n "${data}" ]]; then
    args+=(-H 'Content-Type: application/json' -d "${data}")
  fi
  GH_STATUS="$(curl "${args[@]}" "${STANDBY_RUNNER_API}${path}")"
}

gh_succeeded() {
  [[ "${GH_STATUS}" == 2* ]]
}

# Prints the body of a 2xx response and dies on anything else.
gh_call() {
  gh_api "$@"
  gh_succeeded || die "$1 $2 answered HTTP ${GH_STATUS}: $(head -c 300 "${GH_WORK}/body")"
  cat "${GH_WORK}/body"
}

mint_installation_token() {
  local jwt installation_id
  jwt="$(mint_app_jwt)"
  gh_use_token "${jwt}"
  installation_id="$(gh_call GET /app/installations \
    | jq -er 'if length == 1 then .[0].id else error("expected exactly one installation") end')"
  gh_call POST "/app/installations/${installation_id}/access_tokens" | jq -er .token
}
