#!/usr/bin/env bash
#
# ExecStartPre of standby-runner.service, run as root. Mints a just-in-time
# runner configuration for the org's restricted runner group and hands it to the
# runner through a root-only environment file, so a registration is good for one
# job and no long-lived registration token exists (ADR-0085).

set -euo pipefail

: "${STANDBY_RUNNER_APP_ID:?}" "${STANDBY_RUNNER_KEY_PATH:?}" "${STANDBY_RUNNER_ORG:?}"
: "${STANDBY_RUNNER_GROUP_ID:?}" "${STANDBY_RUNNER_LABEL:?}" "${STANDBY_RUNNER_JIT_ENV:?}"

# shellcheck source=./standby-runner-lib.sh disable=SC1091
source "${STANDBY_RUNNER_LIB:-/usr/local/lib/standby-runner/lib.sh}"

umask 077
GH_WORK="$(mktemp -d)"
trap 'rm -rf "${GH_WORK}"' EXIT

token="$(mint_installation_token)"
gh_use_token "${token}"

request="$(jq -n \
  --arg name "standby-$(hostname)-$(date +%s)" \
  --argjson group "${STANDBY_RUNNER_GROUP_ID}" \
  --arg label "${STANDBY_RUNNER_LABEL}" \
  '{name: $name, runner_group_id: $group, labels: ["self-hosted", "Linux", "X64", $label], work_folder: "_work"}')"

jit_config="$(gh_call POST "/orgs/${STANDBY_RUNNER_ORG}/actions/runners/generate-jitconfig" "${request}" \
  | jq -er .encoded_jit_config)"

printf 'ACTIONS_RUNNER_INPUT_JITCONFIG=%s\n' "${jit_config}" >"${STANDBY_RUNNER_JIT_ENV}"
