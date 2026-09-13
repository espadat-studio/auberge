---
title: "Applications Overview"
---

Auberge deploys a curated stack of self-hosted FOSS applications. Services run natively via systemd; containers are per-app exceptions granted only when upstream supports nothing else (currently Immich).

## Infrastructure

| Application                                        | Description                        |
| -------------------------------------------------- | ---------------------------------- |
| [Caddy](/applications/infrastructure/caddy/)       | Reverse proxy with automatic HTTPS |
| [Cockpit](/applications/infrastructure/cockpit/)   | Web-based server administration    |
| [fail2ban](/applications/infrastructure/fail2ban/) | Intrusion prevention system        |
| [UFW](/applications/infrastructure/ufw/)           | Uncomplicated firewall             |

## Networking

| Application                                      | Description                          |
| ------------------------------------------------ | ------------------------------------ |
| [Blocky](/applications/networking/blocky/)       | DNS server with ad/tracking blocking |
| [Headscale](/applications/networking/headscale/) | Self-hosted Tailscale control server |
| [Tailscale](/applications/networking/tailscale/) | Mesh VPN for secure remote access    |

## Apps

| Application                                    | Description                                 |
| ---------------------------------------------- | ------------------------------------------- |
| [Actual Budget](/applications/apps/actual/)    | Budgeting with EU bank sync                 |
| [Baikal](/applications/apps/baikal/)           | CalDAV/CardDAV server                       |
| [Bichon](/applications/apps/bichon/)           | Email archiving and search                  |
| [Grimmory](/applications/apps/grimmory/)       | Multi-user digital library                  |
| [Calibre](/applications/apps/calibre/)         | Ebook library (alternative to Grimmory)     |
| [Colporteur](/applications/apps/colporteur/)   | Newsletter-to-feed converter                |
| [FreshRSS](/applications/apps/freshrss/)       | RSS feed aggregator                         |
| [Immich](/applications/apps/immich/)           | Photo and video management                  |
| [Navidrome](/applications/apps/navidrome/)     | Music streaming server                      |
| [Paperless-ngx](/applications/apps/paperless/) | Document management system                  |
| [Radio](/applications/apps/radio/)             | Password-gated music streams                |
| [memsearch](/applications/apps/memsearch/)     | Agent memory: markdown store + on-box index |
| [Syncthing](/applications/apps/syncthing/)     | Continuous file synchronization             |
| [Gokapi](/applications/apps/gokapi/)           | Expiring-link file sharing                  |
| [YOURLS](/applications/apps/yourls/)           | URL shortener                               |

## Notifications

| Application                          | Description                               |
| ------------------------------------ | ----------------------------------------- |
| [TGTG Bot](/applications/apps/tgtg/) | Too Good To Go availability notifications |

## AI

| Application                                 | Description                                |
| ------------------------------------------- | ------------------------------------------ |
| [Hermes Agent](/applications/apps/hermes/)  | Self-improving personal AI assistant       |
| [OpenCode](/applications/apps/opencode/)    | Agent runtime on the disposable agent Host |
| [Agent of Empires](/applications/apps/aoe/) | Agent session dashboard, PWA with push     |

## Deployment

All applications are deployed via Ansible playbooks. See [Running Playbooks](/cli-reference/ansible/run/) for details.

```bash
# Deploy all apps
auberge deploy --all

# Deploy specific app
auberge deploy baikal
```

## Backup Support

All applications support backup and restore. See [Backup & Restore](/backup-restore/overview/) for details.
