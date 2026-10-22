#!/usr/bin/env bash
#
# Weekly prune of the Standby Runner's work dir and cargo registry cache. The
# work dir drops entries older than the retention; the cache is dropped whole
# once it passes the cap, since a cold registry only costs one download.

set -euo pipefail

: "${STANDBY_RUNNER_WORK_DIR:?}" "${STANDBY_RUNNER_CARGO_REGISTRY_DIR:?}"
: "${STANDBY_RUNNER_WORK_RETENTION_DAYS:?}" "${STANDBY_RUNNER_CACHE_CAP_MB:?}"

[[ -d "${STANDBY_RUNNER_WORK_DIR}" ]] \
  && find "${STANDBY_RUNNER_WORK_DIR}" -mindepth 1 -maxdepth 1 -mtime "+${STANDBY_RUNNER_WORK_RETENTION_DAYS}" -exec rm -rf {} +

[[ -d "${STANDBY_RUNNER_CARGO_REGISTRY_DIR}" ]] || exit 0
size_mb="$(du -sm "${STANDBY_RUNNER_CARGO_REGISTRY_DIR}" | cut -f1)"
if ((size_mb > STANDBY_RUNNER_CACHE_CAP_MB)); then
  find "${STANDBY_RUNNER_CARGO_REGISTRY_DIR}" -mindepth 1 -delete
fi
