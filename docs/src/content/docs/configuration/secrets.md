---
title: "Secrets Management"
---

Auberge stores all configuration, including sensitive values, in `config.toml`.

## Setup

Generate a `config.toml` scaffold at the XDG config path:

```bash
auberge config init --output "$(auberge config path)"
```

`auberge config init` derives the scaffold from the Key Registry
(`ansible/keys.yml`). Without `--output`, the scaffold is printed to stdout —
useful for piping or inspecting available keys. Pass `--playbooks <a,b,c>`
to emit only the keys required by the named playbooks.

## Setting Config Values

```bash
auberge config set SECRET_NAME value
```

**Required values:**

```bash
auberge config set admin_user_name yourname
auberge config set admin_user_email you@example.com
auberge config set domain example.com
auberge config set cloudflare_dns_api_token your-token
auberge config set ssh_port 22022
```

**Optional values:**

```bash
# Only needed for Cockpit web console login
auberge config set admin_user_password your-linux-password
```

Run `auberge config init` to print the full list of known keys, each with
its documentation string from the Key Registry.

## Password Commands

Values prefixed with `!` are treated as shell commands. When a config value is consumed (during deploy, backup, or other operations), Auberge runs the command via `sh -c` and uses the trimmed stdout as the actual value. The command is **not** evaluated at `config set` time.

**Value formats:**

| Stored value   | Behaviour                                               |
| -------------- | ------------------------------------------------------- |
| `!cmd`         | Runs `sh -c "cmd"`, uses trimmed stdout                 |
| `!!literal`    | Stores as `!literal` (escape hatch - no command is run) |
| `plain string` | Used as-is                                              |

**Example (`config.toml`):**

```toml
baikal_admin_password = "!op read op://vault/baikal/password"
cloudflare_dns_api_token = "secret123"
some_literal_bang = "!!not-a-cmd"
```

**Requirements:** the command must exit 0 and produce non-empty UTF-8 output. The operation fails with a clear error if either condition is not met.

Each distinct command runs at most once per `auberge` invocation: a deploy over several playbooks reuses the first result instead of asking your password manager again. A command whose output should change between uses (a one-time code, for example) gets the same value for the whole invocation. A failed command is not remembered.

A run resolves only the `!` values it reads. Auberge walks the playbook, the roles it reaches (narrowed by `--tags`), `group_vars/` and the inventory, and collects every variable name they use. A `!` value whose key is not among them is never run, so a single-app deploy asks your password manager only for that app's secrets. If a `!` value the run reads fails, the run stops before Ansible starts. The error names the key and shows the command's stderr, never its stdout. Fix the command or remove the key.

Two limits:

- A role that looks a variable up by a computed name reads every key. Today that is blocky, so `deploy infrastructure` still resolves every `!` value.
- DNS API tokens for every Zone still resolve on every run (#1323).

A broken `!` command on a key no run reads goes unnoticed until a run reads it. Plain values (and `!!` escapes) are not filtered.

The command's stdin is closed, but it keeps your terminal, so tools that prompt on `/dev/tty` still work (for example an age YubiKey PIN prompt).

## Viewing Config

```bash
auberge config list
```

## Security

- Never commit `config.toml` to version control - it contains plaintext secrets
- `config.toml` is listed in `.gitignore`
- Run `auberge config init` to print a fresh scaffold derived from the Key Registry

## Troubleshooting

**"Missing required config value"**

```bash
auberge config list              # Check what is set
auberge config set KEY value     # Set if missing
```
