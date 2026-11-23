---
title: "memsearch"
---

Semantic memory for the agent Host: agents accumulate memory across sessions and across worktrees, and it survives the box. Docs: [github.com/zilliztech/memsearch](https://github.com/zilliztech/memsearch). Decision record: [ADR-0064](https://github.com/espadat-studio/auberge/blob/master/meta/adr/0064-agent-memory-pools-in-one-directory.md).

- **URL**: none; a CLI and a library, not a service
- **Data**: `/srv/agent-memory/.memsearch/memory` (markdown, replicated off-box), `~/.memsearch/milvus.db` (index, disposable)
- **Backup**: none, by decision — the agent Host holds no state a backup is the answer for ([ADR-0054](https://github.com/espadat-studio/auberge/blob/master/meta/adr/0054-agent-workloads-run-on-a-dedicated-disposable-host.md))

## Deploy

```bash
auberge ansible run --tags memsearch -H ruche
```

## Configuration

| Key                      | Description                                                              |
| ------------------------ | ------------------------------------------------------------------------ |
| `syncthing_devices`      | The Host's Syncthing peers, `name=DEVICE-ID,...`                         |
| `memsearch_sync_devices` | Names, from `syncthing_devices`, of the peers that hold the off-box copy |

The peer initiates the connection. The Host announces nowhere, so pair it from the peer's side using the Host's tailnet address and port 22000. [`auberge syncthing pair`](/cli-reference/syncthing/pair/) does this on the machine running `auberge`, and again after a rebuild gives the Host a new device ID.

## Notes

The store is markdown and the index is rebuilt from it, so losing the box loses the index and nothing else:

```bash
rm -f ~/.memsearch/milvus.db
memsearch index /srv/agent-memory/.memsearch/memory
```

Each agent worktree keeps its own capture daemon and its own collection over that one shared corpus — an agent in one worktree therefore recalls what an agent in another learned. The reason it is not one collection, and the cost of that, are in ADR-0064.
