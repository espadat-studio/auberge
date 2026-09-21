#!/usr/bin/env bash
#
# tests/forgejo-onboard.test.sh
#
# Unit tests for the pure, network-free logic in examples/forgejo-onboard.sh —
# the derivation of the OAuth redirect URI from a site origin. That derivation
# is the one thing in the script an editor's login fails on silently: Forgejo
# matches a redirect URI exactly, so a missing trailing slash or an accepted
# non-origin registers a URI no site ever serves, and the only symptom is a
# login that will not complete.
#
# The two API steps are not covered here: they are one create and one
# reconcile against a live forge, and stubbing curl would test the stub.
#
# Run: ./tests/forgejo-onboard.test.sh

set -euo pipefail

SUITE_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname -- "${SUITE_DIR}")"
readonly SUITE_DIR REPO_ROOT

# shellcheck source=./assert.sh disable=SC1091
source "${SUITE_DIR}/assert.sh"
# shellcheck source=../examples/forgejo-onboard.sh disable=SC1091
source "${REPO_ROOT}/examples/forgejo-onboard.sh"

# Every refusal below is asserted through a non-zero status, and a call to a
# function that no longer exists is also non-zero. So the subjects are pinned
# first: without this, deleting either one turns its refusals green.
is_defined() { declare -F "$1" >/dev/null; }

printf '== the subjects exist\n'

assert_succeeds 'redirect_uri_for is defined' is_defined redirect_uri_for
assert_succeeds 'repo_name_is_bare is defined' is_defined repo_name_is_bare

printf '\n== redirect_uri_for\n'

assert_eq 'an origin becomes the admin page, trailing slash included' \
  'https://client.example.com/admin/' \
  "$(redirect_uri_for 'https://client.example.com')"

# An origin pasted from a browser carries the slash; registering
# //admin/ would be a URI the site never serves.
assert_eq 'a trailing slash on the origin is not doubled' \
  'https://client.example.com/admin/' \
  "$(redirect_uri_for 'https://client.example.com/')"

assert_eq 'a local dev origin keeps its port' \
  'http://localhost:4321/admin/' \
  "$(redirect_uri_for 'http://localhost:4321')"

assert_eq 'a preview host is an origin like any other' \
  'https://site.example.workers.dev/admin/' \
  "$(redirect_uri_for 'https://site.example.workers.dev')"

printf '\n== redirect_uri_for rejects what is not an origin\n'

assert_fails 'a bare hostname is refused' \
  redirect_uri_for 'client.example.com'

assert_fails 'another scheme is refused' \
  redirect_uri_for 'ftp://client.example.com'

# The refused glob: http*://* also matched httpx:// and httpfoo://.
assert_fails 'a scheme that merely starts with http is refused' \
  redirect_uri_for 'httpx://client.example.com'

assert_fails 'a scheme with no host is refused' \
  redirect_uri_for 'https://'

assert_fails 'an empty argument is refused' \
  redirect_uri_for ''

printf '\n== repo_name_is_bare\n'

assert_succeeds 'a plain name is bare' \
  repo_name_is_bare 'client-content'

assert_succeeds 'dots and underscores inside a name are allowed' \
  repo_name_is_bare 'client.example_content'

# The old signature. Left unrefused it would create a repository whose name
# holds a slash, under a request path that is not the one it reads back.
assert_fails 'the old <github-owner/repo> argument is refused' \
  repo_name_is_bare 'owner/client-content'

# curl resolves these out of the request path, so /repos/owner/.. probes
# /repos/ and a 200 there would read as this repository existing.
assert_fails 'a parent-directory segment is refused' \
  repo_name_is_bare '..'

assert_fails 'a bare dot is refused' \
  repo_name_is_bare '.'

assert_fails 'a leading dot is refused' \
  repo_name_is_bare '.hidden'

assert_fails 'a leading dash is refused' \
  repo_name_is_bare '-content'

assert_fails 'an empty name is refused' \
  repo_name_is_bare ''

report 'forgejo-onboard'
