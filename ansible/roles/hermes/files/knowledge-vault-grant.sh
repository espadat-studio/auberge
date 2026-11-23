#!/usr/bin/env bash
#
# Grants an App the Knowledge Vault through POSIX ACLs (#1334), or probes that
# grant as the App.
#
#   knowledge-vault-grant.sh grant VAULT OPERATOR GRANTEE [SUBTREE...]
#   knowledge-vault-grant.sh probe VAULT [SUBTREE...]
#   knowledge-vault-grant.sh revoke VAULT OPERATOR GRANTEE
#
# grant: read (r-X) on the whole Vault, write (rwX) for GRANTEE and OPERATOR on
# each Vault-relative SUBTREE, access and default entries both. Every subtree is
# validated before anything changes. The Vault outside the subtrees is reset to
# read-only on every run, so a subtree dropped from the list is revoked: the
# grant is what is declared, not what accumulated. Files GRANTEE owns outside
# the subtrees are handed to OPERATOR, since owner bits would otherwise outlive
# a revocation. Prints one line per change, nothing when converged.
#
# revoke: removes every GRANTEE entry from the Vault and its ancestors, and
# hands GRANTEE's files to OPERATOR. The grant once config no longer declares
# one.
#
# probe: run as GRANTEE. Fails unless the Vault root is readable, every subtree
# writable, and nothing outside the subtrees writable.
#
# Symlinks are never followed: setfacl and find walk physically, so a link in
# the Vault cannot carry the grant outside it.

set -euo pipefail

die() {
  printf 'knowledge-vault-grant: %s\n' "$*" >&2
  exit 1
}

resolve_vault() {
  [[ "$1" == /* ]] || die "vault '$1' is not an absolute path"
  [[ -d "$1" ]] || die "vault '$1' is not a directory"
  VAULT="$(realpath -e -- "$1")"
}

resolve_subtrees() {
  SUBTREES=()
  local sub target
  for sub in "$@"; do
    [[ -n "${sub}" ]] || die "empty subtree"
    [[ "${sub}" != /* ]] || die "subtree '${sub}' is absolute; name it relative to the vault"
    [[ "/${sub}/" != */../* ]] || die "subtree '${sub}' contains '..'"
    target="$(realpath -m -- "${VAULT}/${sub}")"
    [[ "${target}" == "${VAULT}"/* ]] || die "subtree '${sub}' resolves to ${target}, outside the vault"
    [[ ! -e "${target}" || -d "${target}" ]] || die "subtree '${sub}' is not a directory"
    SUBTREES+=("${target}")
  done
}

build_prune() {
  PRUNE=()
  local sub
  for sub in "${SUBTREES[@]}"; do
    PRUNE+=(-samefile "${sub}" -o)
  done
  ((${#PRUNE[@]} == 0)) && return
  unset 'PRUNE[-1]'
  PRUNE=('(' "${PRUNE[@]}" ')' -prune -o)
}

walk_acl() {
  local flag="$1" op="$2" access="$3" default="$4"
  shift 4
  find -P "$@" -type f -exec setfacl -P ${flag:+"${flag}"} "${op}" "${access}" {} +
  find -P "$@" -type d -exec setfacl -P ${flag:+"${flag}"} "${op}" "${access}${default:+,${default}}" {} +
}

change_acl() {
  local tested changes
  tested="$(walk_acl --test "$@")"
  changes="$(grep -v '\*,\*$' <<<"${tested}" || true)"
  [[ -n "${changes}" ]] || return 0
  walk_acl "" "$@"
  printf '%s\n' "${changes}" | sed 's/: .*//; s/^/acl /'
}

set_acl() { change_acl -m "$@"; }

untraversable_ancestors() {
  local dir="${VAULT}"
  while dir="$(dirname -- "${dir}")" && [[ "${dir}" != / ]]; do
    (($((8#$(stat -c %a -- "${dir}") & 1)) == 0)) && printf '%s\n' "${dir}"
  done
  return 0
}

hand_over() {
  local operator="$1" grantee="$2"
  shift 2
  find -P "$@" -user "${grantee}" -exec chown -h -- "${operator}:" {} + -printf 'chown %p\n'
}

grant() {
  (($# >= 3)) || die "usage: grant VAULT OPERATOR GRANTEE [SUBTREE...]"
  resolve_vault "$1"
  local operator="$2" grantee="$3" dir part sub
  shift 3
  id -u -- "${operator}" >/dev/null || die "no user '${operator}'"
  id -u -- "${grantee}" >/dev/null || die "no user '${grantee}'"
  resolve_subtrees "$@"

  while read -r dir; do
    set_acl "u:${grantee}:--x" "" "${dir}" -maxdepth 0
  done < <(untraversable_ancestors)

  for sub in "${SUBTREES[@]}"; do
    dir="${VAULT}"
    local -a parts
    IFS=/ read -ra parts <<<"${sub#"${VAULT}"/}"
    for part in "${parts[@]}"; do
      dir="${dir}/${part}"
      [[ -e "${dir}" ]] && continue
      mkdir -- "${dir}"
      chown -h -- "${operator}:" "${dir}"
      printf 'created %s\n' "${dir}"
    done
  done

  build_prune
  hand_over "${operator}" "${grantee}" "${VAULT}" "${PRUNE[@]}"
  set_acl "u:${grantee}:rX" "d:u:${grantee}:rX" "${VAULT}" "${PRUNE[@]}"
  for sub in "${SUBTREES[@]}"; do
    set_acl "u:${grantee}:rwX,u:${operator}:rwX" "d:u:${grantee}:rwX,d:u:${operator}:rwX" "${sub}"
  done
}

revoke() {
  (($# == 3)) || die "usage: revoke VAULT OPERATOR GRANTEE"
  resolve_vault "$1"
  local operator="$2" grantee="$3" dir
  id -u -- "${operator}" >/dev/null || die "no user '${operator}'"
  id -u -- "${grantee}" >/dev/null || die "no user '${grantee}'"
  while read -r dir; do
    change_acl -x "u:${grantee}" "" "${dir}" -maxdepth 0
  done < <(untraversable_ancestors)
  hand_over "${operator}" "${grantee}" "${VAULT}"
  change_acl -x "u:${grantee}" "d:u:${grantee}" "${VAULT}"
}

probe() {
  (($# >= 1)) || die "usage: probe VAULT [SUBTREE...]"
  resolve_vault "$1"
  shift
  resolve_subtrees "$@"
  [[ -r "${VAULT}" && -x "${VAULT}" ]] || die "cannot read ${VAULT}"
  local sub leaks
  for sub in "${SUBTREES[@]}"; do
    [[ -d "${sub}" && -w "${sub}" && -x "${sub}" ]] || die "cannot write ${sub}"
  done
  build_prune
  leaks="$(find -P "${VAULT}" "${PRUNE[@]}" ! -type l -writable -print)"
  [[ -z "${leaks}" ]] || die "can write outside the declared subtrees:"$'\n'"${leaks}"
}

case "${1:-}" in
  grant | probe | revoke)
    command="$1"
    shift
    "${command}" "$@"
    ;;
  *) die "usage: $0 grant|probe|revoke ..." ;;
esac
