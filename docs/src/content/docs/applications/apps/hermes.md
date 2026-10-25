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

Optional: `hermes_exa_api_key` (web search), `hermes_telegram_allowed_users` (comma-separated user IDs — restricts bot access; without this any user knowing the token can interact).

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
