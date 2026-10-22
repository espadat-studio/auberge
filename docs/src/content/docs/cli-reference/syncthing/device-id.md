---
title: "auberge syncthing device-id"
---

Print an agent Host's Syncthing device ID

## Synopsis

```bash
auberge syncthing device-id [-H <HOST>]
```

## Alias

`auberge st id`

## Description

Reads the Host's device ID over SSH from its Syncthing REST API (`/rest/system/status`, `myID`). The API key comes from the `syncthing` user's `config.xml`, read with `sudo -n` because that directory is `0750`.

Stdout carries the ID and nothing else. Progress and errors go to stderr.

## Options

| Option       | Description                                                  | Default  |
| ------------ | ------------------------------------------------------------ | -------- |
| `-H, --host` | Host running Syncthing (prompted on a TTY, required off one) | prompted |

## Examples

```bash
auberge syncthing device-id -H ruche

ID=$(auberge st id -H ruche)
```
