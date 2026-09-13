---
title: "Tailscale"
---

Mesh VPN for secure remote access built on WireGuard.

Official Documentation: [https://tailscale.com](https://tailscale.com)

## Deployment

```bash
auberge ansible run --tags tailscale
```

## Configuration

Enrollment keys are [minted per run](/applications/networking/headscale/) against self-hosted Headscale; `tailscale_authkey` in `config.toml` is a fallback for the host that serves it, and the source on a Tailscale SaaS tailnet. See [Environment Variables](/configuration/environment-variables/).

### Optional: DNS Automation

Setting `tailscale_api_key` in `config.toml` enables automated DNS configuration. The Blocky role uses this key to register itself as the tailnet DNS nameserver via the Tailscale API, routing all tailnet client DNS queries through Blocky for ad-blocking.

See [Blocky - Required config](/applications/networking/blocky/#required-config) for details.

### Server DNS Behavior

The server runs with `accept_dns: false` (the default) to prevent Tailscale from overriding the server's own DNS resolution. This avoids DNS breakage during redeployments — if the server accepted Tailscale DNS pointing at its own Blocky instance, a Blocky restart would break DNS resolution for the server itself.

## Self-Hosted Control Server

To replace Tailscale SaaS with a self-hosted control plane, see [Headscale](/applications/networking/headscale/). Set `tailscale_login_server` in `config.toml` to point nodes at your Headscale instance.

## Related

- [Headscale](/applications/networking/headscale/)
- [Blocky](/applications/networking/blocky/)
- [Environment Variables](/configuration/environment-variables/)
- [Applications Overview](/applications/overview/)
