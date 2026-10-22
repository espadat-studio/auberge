---
title: "auberge syncthing pair"
---

Pair this machine's Syncthing with an agent Host

## Synopsis

```bash
auberge syncthing pair [-H <HOST>] [--path <DIR>] [--config-xml <FILE>] [-n] [-y]
```

## Alias

`auberge st p`

## Description

The Host cannot dial out (ADR-0055), so the peer initiates. The peer is the machine running `auberge`, and its Syncthing must be running locally.

`pair` reads the Host's device ID (see [`device-id`](/cli-reference/syncthing/device-id/)), then, through the local Syncthing REST API:

- adds or updates the Host as a device at `tcp://<tailscale_ip>:22000`
- shares the `agent-memory` folder (`sendreceive`) with it
- replaces a stale device of the same name, which a rebuilt Host leaves behind

A rerun on a converged setup writes nothing and says so. Device IDs are compared dash-stripped and upper-cased.

Refuses, writing nothing, when:

- the Host has no cached `tailscale_ip` (run `auberge host detect-tailscale-ip`)
- the folder path is inside, or contains, another local Syncthing folder
- `agent-memory` already exists at a different path
- `agent-memory` is already shared with a device that is neither this machine nor the Host

## Options

| Option          | Description                                                   | Default                               |
| --------------- | ------------------------------------------------------------- | ------------------------------------- |
| `-H, --host`    | Agent Host to pair with (prompted on a TTY, required off one) | prompted                              |
| `--path`        | Local folder path                                             | `~/memories/<host>`                   |
| `--config-xml`  | Local Syncthing `config.xml` holding the API key              | `~/.local/state/syncthing/config.xml` |
| `-n, --dry-run` | Show what would change and write nothing                      | off                                   |
| `-y, --yes`     | Skip the prompt before removing a stale device                | off                                   |

## Examples

```bash
auberge syncthing pair -H ruche

# Preview first
auberge st p -H ruche --dry-run

# After rebuilding ruche, in a script
auberge st p -H ruche -y
```
