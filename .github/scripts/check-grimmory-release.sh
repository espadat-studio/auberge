#!/usr/bin/env bash
#
# check-grimmory-release.sh — assert the pinned Grimmory version has a Built Release.
#
# The jar is built by a manual `build-grimmory` dispatch and published as the
# `grimmory/v<version>` release of this repo. A tag alone is not enough: a
# half-failed run can leave one without the jar.
#
# Exit codes:
#   0 — release carries a non-empty grimmory-api.jar
#   1 — release or asset missing

set -euo pipefail

readonly META='ansible/playbooks/grimmory.meta.yml'
readonly ASSET='grimmory-api.jar'

version=$(awk '/^version:/ {inside=1; next} inside && $1 == "value:" {gsub(/"/, "", $2); print $2; exit}' "${META}")
[[ -n "${version}" ]] || {
  echo "no version.value in ${META}" >&2
  exit 1
}

repo="${GITHUB_REPOSITORY:-espadat-studio/auberge}"
size=$(gh api "repos/${repo}/releases/tags/grimmory/v${version}" \
  --jq ".assets[] | select(.name == \"${ASSET}\") | .size" 2>/dev/null || true)

if [[ "${size:-0}" -gt 0 ]]; then
  echo "grimmory/v${version}: ${ASSET} ${size} bytes"
  exit 0
fi

echo "::error::grimmory/v${version} has no non-empty ${ASSET}" >&2
echo "Build it first: gh workflow run build-grimmory.yml -f version=${version}" >&2
exit 1
