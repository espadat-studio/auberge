---
title: "auberge host remove"
---

Remove a host from configuration

## Synopsis

```bash
auberge host remove [OPTIONS] [NAME]
# Alias: auberge h rm
```

## Description

Removes a host from the Auberge configuration. Prompts for confirmation unless --yes flag is used.

If `NAME` is omitted, you'll be prompted to select a host.

Before the roster entry goes, the host's headscale node is deleted on the roster host serving headscale. That revokes the tag reach the node was granted. The node is matched by the host's cached `tailscale_ip`, or by name when none is cached. A node holding the cached address under a different name is not deleted: the address may be stale. The warning prints the exact `headscale nodes delete` command to run once you have checked. If headscale cannot be reached, nothing is removed; the error names the escape for a coordinator that is gone. A roster with no headscale host skips this step.

**Warning**: It does not:

- Delete the VPS itself
- Remove SSH keys
- Clean up backups
- Delete DNS records pointing at the host. `auberge dns status` flags them afterwards as off-roster records.

## Arguments

| Argument | Description                               |
| -------- | ----------------------------------------- |
| NAME     | Host name to remove (omit to be prompted) |

## Options

| Option    | Description              | Default |
| --------- | ------------------------ | ------- |
| -y, --yes | Skip confirmation prompt | false   |

## Examples

```bash
# Remove with confirmation
auberge host remove myserver

# Interactive (prompts for host)
auberge host remove

# Skip confirmation (for scripts)
auberge host remove old-server --yes
```

## Interactive Session Example

```bash
$ auberge host remove myserver
Remove host 'myserver' and delete its headscale node? [y/N]: y
✓ Deleted headscale node 'myserver' (id 4)
✓ Host 'myserver' removed
```
