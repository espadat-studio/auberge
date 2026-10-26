#!/usr/bin/env bash
#
# First ExecStartPre of standby-runner.service, run as root. Gives every job the
# same starting state, so one runner-group repo's job cannot read another's
# checkout or leave code behind for a later one (ADR-0085):
#
# - The release is root-owned and job code cannot write it. This step is the
#   runner's self-update: it moves the release to GitHub's latest, and only to a
#   download whose sha256 matches the digest GitHub published for the asset.
# - The job dir is rebuilt from the release as hard links. The files stay
#   root-owned and read-only; only the dir itself, where the runner writes its
#   registration, logs and _work, belongs to the runner user.
# - HOME is emptied, caches and toolchains included, and closed again.

set -euo pipefail

: "${STANDBY_RUNNER_APP_ID:?}" "${STANDBY_RUNNER_KEY_PATH:?}"
: "${STANDBY_RUNNER_RELEASE_DIR:?}" "${STANDBY_RUNNER_JOB_DIR:?}" "${STANDBY_RUNNER_HOME:?}"
: "${STANDBY_RUNNER_USER:?}" "${STANDBY_RUNNER_GROUP:?}"

# shellcheck source=./standby-runner-lib.sh disable=SC1091
source "${STANDBY_RUNNER_LIB:-/usr/local/lib/standby-runner/lib.sh}"

GH_WORK="$(mktemp -d)"
trap 'rm -rf "${GH_WORK}"' EXIT
umask 022

install_release() {
  local release="$1" version="$2" name asset url digest staging="${STANDBY_RUNNER_RELEASE_DIR}.new"
  name="actions-runner-linux-x64-${version}.tar.gz"
  asset="$(jq -ec --arg name "${name}" '.assets[] | select(.name == $name)' <<<"${release}")"
  url="$(jq -er .browser_download_url <<<"${asset}")"
  digest="$(jq -er '.digest // empty | select(startswith("sha256:")) | ltrimstr("sha256:")' <<<"${asset}")" \
    || die "${name} has no published sha256 digest"
  curl -fsSL -o "${GH_WORK}/${name}" "${url}"
  printf '%s  %s\n' "${digest}" "${GH_WORK}/${name}" | sha256sum -c --status \
    || die "${name} does not match its published digest"
  rm -rf "${staging}"
  mkdir "${staging}"
  tar -xzf "${GH_WORK}/${name}" -C "${staging}" --no-same-owner --no-same-permissions
  chmod -R go-w "${staging}"
  printf '%s\n' "${version}" >"${staging}/.version"
  rm -rf "${STANDBY_RUNNER_RELEASE_DIR}"
  mv "${staging}" "${STANDBY_RUNNER_RELEASE_DIR}"
}

token="$(mint_installation_token)"
gh_use_token "${token}"
release="$(gh_call GET /repos/actions/runner/releases/latest)"
version="$(jq -er '.tag_name | ltrimstr("v")' <<<"${release}")"
[[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "unexpected runner release tag: ${version}"

installed="${STANDBY_RUNNER_RELEASE_DIR}/.version"
if ! [[ -f "${installed}" && "$(<"${installed}")" == "${version}" ]]; then
  install_release "${release}" "${version}"
fi

# Emptied and refilled in place, never removed: a ReadWritePaths entry is a
# mount point inside the unit, even for this root step, and rm fails with EBUSY.
find "${STANDBY_RUNNER_JOB_DIR}" -mindepth 1 -delete
cp -al "${STANDBY_RUNNER_RELEASE_DIR}/." "${STANDBY_RUNNER_JOB_DIR}"
chown "${STANDBY_RUNNER_USER}:${STANDBY_RUNNER_GROUP}" "${STANDBY_RUNNER_JOB_DIR}"
chmod 0750 "${STANDBY_RUNNER_JOB_DIR}"

find "${STANDBY_RUNNER_HOME}" -mindepth 1 -delete
chmod 0700 "${STANDBY_RUNNER_HOME}"
