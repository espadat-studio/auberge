---
title: "Auberge"
description: "Self-host a full FOSS stack on a 2 GB VPS as plain systemd services. Rust CLI over Ansible to deploy, back up and restore it."
---

> Self-host a full FOSS stack on a 2 GB VPS. Deploy it with one command, rebuild it on a fresh box with another.

Auberge is a Rust CLI that runs Ansible playbooks to install a full self-hosted stack as plain systemd services: RSS reader, budgeting, calendar and contacts, documents, music, file sync, DNS with ad-blocking, a Tailscale mesh. There is no Docker layer underneath, so it fits a small box. Every version is pinned in the repo and Renovate keeps it current. If the host dies, `auberge restore` brings it back on a new one.

```bash
cargo install auberge
auberge host add my-vps 203.0.113.10
auberge deploy --all --host my-vps
```

That's it. Auberge configures hardening, infrastructure, and applications.

## What you get

| Layer          | Components                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Hardening      | UFW, fail2ban, kernel sysctl, SSH on a custom port                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Infrastructure | [Caddy](/applications/infrastructure/caddy/) (auto-HTTPS), [Tailscale](/applications/networking/tailscale/), [Blocky](/applications/networking/blocky/) (DNS + ad-blocking), [Headscale](/applications/networking/headscale/)                                                                                                                                                                                                                                                                                                                                     |
| Apps           | [Actual Budget](/applications/apps/actual/), [Baikal](/applications/apps/baikal/), [Bichon](/applications/apps/bichon/), [FreshRSS](/applications/apps/freshrss/), [Navidrome](/applications/apps/navidrome/), [Calibre](/applications/apps/calibre/), [Grimmory](/applications/apps/grimmory/), [Paperless-ngx](/applications/apps/paperless/), [Gokapi](/applications/apps/gokapi/), [YOURLS](/applications/apps/yourls/), [Syncthing](/applications/apps/syncthing/), [memsearch](/applications/apps/memsearch/), [Colporteur](/applications/apps/colporteur/) |
| Notifications  | [TGTG Bot](/applications/apps/tgtg/) (Too Good To Go via Telegram)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| AI             | [Hermes](/applications/apps/hermes/) (self-improving Telegram agent)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |

## Where to start

- [Quick Start](/getting-started/quick-start/) — 5-minute walkthrough
- [First Deployment](/getting-started/first-deployment/) — full setup with config + DNS
- [CLI Reference](/cli-reference/auberge/) — every command
- [Backup & Restore](/backup-restore/overview/) — data protection and migration

## Requirements

- VPS with root/sudo access (Linux, 2 GB RAM minimum, 4 GB recommended with Grimmory)
- Linux or macOS workstation (Windows not supported — use WSL2)
- Cloudflare account (optional, for managed DNS)

## Philosophy

_Selfware_ — direct control, no abstraction layers, transparent operations.

[GitHub](https://github.com/espadat-studio/auberge) · [Issues](https://github.com/espadat-studio/auberge/issues) · [Contributing](/development/contributing/)
