#!/usr/bin/env bash
#
# tests/recompute-checksums.test.sh
#
# Unit tests for .github/scripts/recompute-checksums.sh: how a co-pinned URL
# renders from its defaults file and Playbook Meta, and how a new URL gets its
# digest written beside it.
#
# No network: curl is a stub on PATH that writes fixed bytes to its -o target.
#
# Run: ./tests/recompute-checksums.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"
# shellcheck source=../.github/scripts/recompute-checksums.sh disable=SC1091
source "${REPO_ROOT}/.github/scripts/recompute-checksums.sh"

readonly FIXTURES="${WORKDIR}/fixtures"
mkdir -p "${FIXTURES}/bin"

cat >"${FIXTURES}/bin/curl" <<'STUB'
#!/usr/bin/env bash
while (($#)); do
  [[ "$1" == -o ]] && out="$2"
  shift
done
printf 'asset bytes' >"${out}"
STUB
chmod +x "${FIXTURES}/bin/curl"
PATH="${FIXTURES}/bin:${PATH}"

cat >"${FIXTURES}/defaults.yml" <<'YAML'
demo_java_version: "25.0.5+8"
demo_java_archive: "jre_{{ demo_java_version | replace('+', '_') }}.tar.gz"
demo_java_url: "https://example.test/jdk-{{ demo_java_version }}/{{ demo_java_archive }}"
demo_java_sha256: "0000000000000000000000000000000000000000000000000000000000000000"
demo_archive_url: "https://example.test/v{{ demo_version }}/demo.tar.gz"
demo_key_url: "https://example.test/{{ ansible_facts['distribution_release'] }}.gpg"
YAML

cat >"${FIXTURES}/meta.yml" <<'YAML'
version:
  value: "1.2.3"
YAML

render() {
  render_url "${FIXTURES}/defaults.yml" "${FIXTURES}/meta.yml" demo "$(raw_value "${FIXTURES}/defaults.yml" "$1")"
}

assert_eq "a replace filter applies to the variable it follows" \
  "https://example.test/jdk-25.0.5+8/jre_25.0.5_8.tar.gz" "$(render demo_java_url)"

assert_eq "the App Version resolves from the Playbook Meta" \
  "https://example.test/v1.2.3/demo.tar.gz" "$(render demo_archive_url)"

assert_fails "a reference outside the file's own variables is refused" \
  render demo_key_url

recompute_digest "${FIXTURES}/defaults.yml" "" "${FIXTURES}/meta.yml" "" demo demo_java_sha256 \
  "0000000000000000000000000000000000000000000000000000000000000000" >/dev/null
assert_eq "a new URL gets the digest of the bytes it serves" \
  "demo_java_sha256: \"$(printf 'asset bytes' | sha256sum | cut -d' ' -f1)\"" \
  "$(grep '^demo_java_sha256:' "${FIXTURES}/defaults.yml")"

report recompute-checksums
