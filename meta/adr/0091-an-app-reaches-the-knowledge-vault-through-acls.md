# ADR-0091: An App reaches the Knowledge Vault through ACLs, held by `ignorePerms`

## Status

Accepted, 2026-11-23. Issue #1334.

## Decision

**An App's grant into the Knowledge Vault is POSIX ACLs on the Vault in place. The Vault's Syncthing folder runs with `ignorePerms: true`, and the role pins `syncXattrs` off.**

- `hermes_knowledge_vault_write_paths` declares the grant. Unset means no access, `""` means read-only, and a comma-separated list names the Vault-relative subtrees hermes may write.
- hermes gets `r-X` on the whole Vault. On each subtree, hermes and the operator get `rwX`, as access and default entries. Each ancestor hermes cannot traverse gets `--x` only.
- Every deploy resets the Vault outside the subtrees to read-only, then grants the subtrees. A dropped subtree is revoked: the grant is what is declared, not what accumulated. Files hermes owns outside the subtrees are handed to the operator, because owner bits would otherwise outlive a revocation.
- With `knowledge_vault_path` set and no grant declared, every deploy removes hermes's entries from the Vault and its ancestors. Removing the Vault path itself leaves them: the role no longer knows where the Vault was.
- Subtrees are validated before anything changes: an absolute path, `..`, or a path resolving outside the Vault fails the deploy. A missing subtree is created, owned by the operator.
- A probe runs as hermes after the grant. It fails the deploy unless hermes can read the Vault root, can write every subtree, and can write nothing else.
- Preflight refuses a write grant on a Host with no `knowledge_vault_path`, and a Vault path without its folder ID and devices.
- The walk is `setfacl -P` over `find -P`, never `ansible.posix.acl`. With `recursive`, that module stops at the first unchanged line of `setfacl --test`, so it never reaches files below a Vault root that already holds the entry. Neither walk follows a symlink, so a link in the Vault cannot carry the grant outside it.

## Why `ignorePerms` is load-bearing

From Syncthing source (master @992ae89):

| Behaviour                     | `ignorePerms: false`                                                  | `ignorePerms: true`                               |
| ----------------------------- | --------------------------------------------------------------------- | ------------------------------------------------- |
| Pulled file create mode       | `remote\|0600`, then chmod to remote (`folder_sendrecv.go:1707-1710`) | `0666`, no chmod (`sharedpullerstate.go:164-169`) |
| Dir create mode               | remote, then chmod (`folder_sendrecv.go:637-649`)                     | `0777`, no chmod                                  |
| Metadata-only update          | re-chmods existing files and dirs (`:668-669`, `:1329-1330`)          | skipped                                           |
| Effective ACL mask for hermes | `r--` on a `644` file                                                 | `rw-`                                             |

The chmod rewrites the ACL mask from the group bits, so without `ignorePerms` every file the laptop touches drops hermes back to read-only. With it, a new file's mode comes from the default ACL. `ignorePerms: true` also announces `NoPermissions` (`lib/scanner/walk.go:470,520`), so peers need no change.

`syncXattrs` must stay off. When on, Syncthing strips local extended attributes that the peer lacks, and POSIX ACLs are extended attributes (`lib/fs/basicfs_xattr_unix.go:137-141`).

## Alternatives rejected

- **Move the Vault to `/srv` with a shared group.** It still needs `ignorePerms`, because mixed modes defeat a group too, and it moves the folder path on every device.
- **A shared group in place.** File modes in the Vault are mixed (`644` and `664`), because Syncthing replicates the laptop's modes. A group grants write on only some files.

## Trade-offs

- **Every deploy rewalks the Vault.** `setfacl --test` runs over every file. It reports a change only when an entry differs, so a converged Host reports none.
- **The gateway sees `nobody:nogroup`.** `PrivateTmp=true` implies `PrivateUsers=` for a user unit, so every owner but `hermes` maps to `nobody`. The kernel still checks real UIDs, so this only changes what `ls -l` shows.
- **The folder ID moved.** The live folder's legacy ID is replaced by the one config declares. Syncthing cannot rename an ID, so the operator removes it on every device before the first deploy.

## Consequences

`tests/knowledge-vault-grant.test.sh` pins validation, revocation, symlink handling and the probe's verdict. `src/services/required_keys.rs` holds the preflight. The Syncthing role compares `ignorePerms`, `syncXattrs` and versioning before every folder write, so a change made in the web UI is reverted on the next deploy.
