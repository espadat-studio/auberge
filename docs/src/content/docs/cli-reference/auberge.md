---
title: "auberge"
---

CLI for self-hosted infrastructure management.

```bash
auberge [GLOBAL OPTIONS] <COMMAND>
```

## Global options

| Option                    | Description                                                                                                                                                            |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `-v, --verbose`           | Stream subprocess output (kept on failure, dimmed on success)                                                                                                          |
| `-q, --quiet`             | Suppress chrome on stderr (errors and stdout data unchanged); mutually exclusive with `--verbose`                                                                      |
| `--no-color`              | Disable colored output (or set `NO_COLOR` env var)                                                                                                                     |
| `--via <public\|tailnet>` | Reach hosts over their `public` or `tailnet` address for this run, overriding each host's `prefer_tailnet`; see [Tailnet Transport](/configuration/tailnet-transport/) |
| `-h, --help`              | Print help                                                                                                                                                             |
| `-V, --version`           | Print version                                                                                                                                                          |

## Commands

| Command                                            | Alias | Purpose                                                  |
| -------------------------------------------------- | ----- | -------------------------------------------------------- |
| [deploy](/cli-reference/deploy/)                   | `dp`  | Deploy apps with auto-hardening                          |
| [versions](/cli-reference/versions/)               | `v`   | Report declared App and Tool Versions and upstream drift |
| [ansible](/cli-reference/ansible/run/)             | `a`   | Run Ansible playbooks                                    |
| [backup](/cli-reference/backup/create/)            | `b`   | Backup / restore / push / prune / verify                 |
| [dns](/cli-reference/dns/list/)                    | `d`   | Cloudflare DNS management                                |
| [host](/cli-reference/host/add/)                   | `h`   | Manage `hosts.toml`                                      |
| [ssh](/cli-reference/ssh/keygen/)                  | `ss`  | SSH key generation and deployment                        |
| [sync](/cli-reference/sync/music/)                 | `sy`  | rsync media to the VPS                                   |
| [headscale](/cli-reference/headscale/add-user/)    | `hs`  | Headscale users and nodes                                |
| [bichon](/cli-reference/bichon/reconcile-folders/) | —     | Bichon folder reconciliation                             |
| [config](/cli-reference/config/overview/)          | `c`   | Manage `config.toml`                                     |
| [select](/cli-reference/select/host/)              | `se`  | Interactive host / playbook pickers                      |
| [completions](/cli-reference/completions/)         | —     | Generate shell completion script                         |

## Files

| Purpose  | Path                              |
| -------- | --------------------------------- |
| Hosts    | `~/.config/auberge/hosts.toml`    |
| Config   | `~/.config/auberge/config.toml`   |
| Backups  | `~/.local/share/auberge/backups/` |
| SSH keys | `~/.ssh/identities/`              |

See [Configuration](/configuration/hosts/) for details on the config files.

## Examples

```bash
auberge host add my-vps 203.0.113.10
auberge deploy --all --host my-vps
auberge backup create --host my-vps
auberge dns list
```
