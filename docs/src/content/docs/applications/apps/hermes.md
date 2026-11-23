---
title: "Hermes"
---

Self-improving personal AI assistant (Nous Research) that connects to Telegram and routes queries to your chosen LLM provider. Docs: [hermes-agent.nousresearch.com](https://hermes-agent.nousresearch.com/docs)

- **URL**: no public URL — outbound polling only, no inbound port
- **Data**: `/home/hermes/.hermes/` on the VPS
- **Runs as**: the `hermes` user, with no sudo and no SSH login (ADR-0087)

## Deploy

```bash
auberge deploy hermes
```

Dependency layers (hardening, infrastructure) resolved automatically.

## Required config

| Key                         | Purpose                                                                              |
| --------------------------- | ------------------------------------------------------------------------------------ |
| `hermes_llm_provider`       | Provider slug: `anthropic`, `deepseek`, `gemini`, `kimi`, `openrouter`, `xai`, `zai` |
| `hermes_llm_api_key`        | API key for the chosen provider                                                      |
| `hermes_telegram_bot_token` | Bot token from [@BotFather](https://t.me/BotFather)                                  |

Optional: `hermes_exa_api_key` (web search), `hermes_intervals_api_key` (installs [intervals-mcp-server](https://github.com/mvilanova/intervals-mcp-server) into `~/.hermes/intervals-mcp` and writes `INTERVALS_API_KEY` to `.env`; see below), `hermes_telegram_allowed_users` (comma-separated user IDs — restricts bot access; without this any user knowing the token can interact).

## Notes

:::tip
`hermes_llm_provider` must match the provider set in `~/.config/hermes/config.yaml`, which is synced to the VPS on deploy.
:::

After deploy, verify the service and send a test message to your bot:

```bash
sudo systemctl --user --machine=hermes@ status hermes-gateway
sudo journalctl _UID=$(id -u hermes) -f
```

For interactive commands such as `hermes skills`, use `sudo -iu hermes` on the host.

Secrets stored in `/home/hermes/.hermes/.env` (mode `0600`). No public ports exposed.

:::note
Before ADR-0087 the gateway ran as the admin user. The first deploy after it stops and removes that unit, and starts fresh under `hermes`. The old `~/.hermes/` in the admin home is left in place. Copy any sessions or memories you want to keep into `/home/hermes/.hermes/` and fix ownership, then delete the old directory.
:::

## Intervals.icu

With `hermes_intervals_api_key` set, declare the server in `config.yaml`:

```yaml
mcp_servers:
  intervals:
    command: "${userHome}/.hermes/intervals-mcp/bin/python"
    args: [-m, intervals_mcp_server.server]
    env:
      API_KEY: "${INTERVALS_API_KEY}"
      ATHLETE_ID: "<id>"
```

The role pins the server to `hermes_intervals_mcp_ref`. Renovate does not track it: upstream has no tags.

## Knowledge Vault

hermes can read the operator's _Knowledge Vault_ and write the subtrees config names. The decision record is [ADR-0091](https://github.com/espadat-studio/auberge/blob/master/meta/adr/0091-an-app-reaches-the-knowledge-vault-through-acls.md).

```toml
[hosts.my-host]
knowledge_vault_path = "/home/me/notes"
knowledge_vault_folder_id = "knowledge"
knowledge_vault_devices = "laptop,phone"
syncthing_devices = "laptop=XXXXXXX-...,phone=YYYYYYY-..."
hermes_knowledge_vault_write_paths = "journal,inbox/agent"
```

| `hermes_knowledge_vault_write_paths` | Grant                                     |
| ------------------------------------ | ----------------------------------------- |
| unset                                | none                                      |
| `""`                                 | read the whole Vault                      |
| `a,b/c`                              | read the whole Vault, write `a` and `b/c` |

- Subtrees are Vault-relative. An absolute path, `..`, or a path resolving outside the Vault fails the deploy before anything changes.
- A missing subtree is created, owned by the admin user.
- A subtree dropped from the list goes back to read-only on the next deploy. Files hermes owns outside the subtrees are handed to the admin user, so owner bits cannot outlive a revocation.
- Unsetting `hermes_knowledge_vault_write_paths` while `knowledge_vault_path` stays set removes every hermes entry on the next deploy. Unset the grant and deploy before removing the Vault path, or the entries stay.
- The grant is POSIX ACLs. Each ancestor hermes cannot traverse gets `--x` only, so hermes lists nothing in the admin home.
- After the grant, a probe runs as hermes. It fails the deploy unless hermes can read the Vault root, can write each subtree, and can write nothing else.
- The gateway gets `KNOWLEDGE_VAULT_PATH` in its environment.

Setting `knowledge_vault_path` also declares the Vault's Syncthing folder for the admin user's Syncthing, with `ignorePerms` and 30-day staggered versioning. Deploy it with `auberge deploy syncthing`. To move an existing folder to a new ID, remove the old ID on every device before deploying, then accept the new share on each peer at its existing path.

:::caution
Inside the gateway every owner except `hermes` shows as `nobody:nogroup`: the unit's `PrivateTmp=true` implies `PrivateUsers=`. This only changes what `ls -l` shows. The kernel still checks real UIDs, so the ACLs work.
:::
