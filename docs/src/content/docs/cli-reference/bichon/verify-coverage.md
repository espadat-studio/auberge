---
title: "auberge bichon verify-coverage"
---

## Synopsis

```bash
auberge bichon verify-coverage [--host <HOST>] [--account <EMAIL>] [--folder <NAME>] [--before <YYYY-MM-DD>] [OPTIONS]
```

## Description

Proves, message by message, that the Email Archive covers one folder's mail older than a cutoff — by identity, not by count.

Counts are not identities: the archive is append-only, so after a folder's first expunge it permanently out-counts the mailbox for any window, and a count-based check passes on surplus while specific messages have no archived counterpart. The one class of mail that produces such messages is exactly the one the archive's hourly run cannot see: mail entering a Synced Folder below the [Archive Cursor](/cli-reference/bichon/rescan/) (moved, bulk-imported, or backdated — a message-date watermark misses all three).

Bichon's Internal Store syncs by UID and has no date blindspot, so it knows that mail. The check compares the store's in-window envelopes for the folder against the canonical `message_id` of every sidecar in the account (ADR-0013, ADR-0088) and names every store message the archive cannot vouch for.

Evidence is per account, not per folder. A sidecar's `folder` records where the body was first seen and is used only to restore it. A message moved after archiving keeps that one sidecar, so its new folder is covered by it.

A message with no `Message-ID` header has a synthetic id in the store that regenerates on re-import. The archive keys its body as `sha256:<hash of the .eml>` instead. For each such message in the window, the command downloads the body from Bichon and hashes it the same way. It is matched when any sidecar in the account holds that key. Otherwise it is listed under `missing`, named by that key. Byte-identical bodies share one key, so duplicate sidecars of one body vouch for nothing else.

A Message-ID claimed more than once is also unverifiable by identity. That covers two store messages in the account sharing one, or two archived bodies sharing one. Claimants are counted across every folder and date, not just the window: a self-sent mail's Sent copy cannot lend its archived body to the INBOX copy. Any sender can write any Message-ID, so a match cannot show which body belongs to which message. Each such id is listed under `unverifiable.contested` with both counts, and is a gap when the store count is higher.

Refuses to answer (exit 2) when a scoped sidecar lacks `message_id` — that is an unknown, not a zero — naming the file; one `bichon-archive.service` run backfills it.

What this does **not** prove: that the live Upstream Mailbox holds nothing the store has never ingested. This command cannot reach IMAP. It reports the bound instead: `store_internal_date_count` is the folder's raw store message count with an IMAP internal date before the cutoff. `examples/bichon-expunge.sh` refuses when its IMAP `SEARCH BEFORE` count exceeds it (ADR-0088). Both sides count raw messages on the same date, so a folder holding one Message-ID twice counts it twice on each side.

`store_messages` and `store_internal_date_count` can differ. The first windows on the `Date:` header, which the sender writes; the second on the date the server received the message. Bichon records a missing internal date as 0, which falls inside every window, as it does for Bichon's own filter. Such messages are counted, and `store_without_internal_date` lists their uids. `examples/bichon-expunge.sh` refuses unless every one of them is in its own IMAP `SEARCH BEFORE` result, since any other would inflate the store count.

## Options

| Option              | Description                                           | Default                   |
| ------------------- | ----------------------------------------------------- | ------------------------- |
| -H, --host HOST     | Target host running Bichon                            | prompted                  |
| --account EMAIL     | Account email; also names the Email Archive directory | prompted                  |
| --folder NAME       | Folder whose coverage to verify (exact, case matters) | prompted                  |
| --before YYYY-MM-DD | Verify messages dated strictly before this UTC date   | prompts if omitted        |
| --archive-path PATH | Email Archive root on the host                        | `/var/lib/bichon-archive` |
| -o, --output FORMAT | Output format (`human`, `json`)                       | `human`                   |

Omit any of the first three on a terminal and a picker is drawn for it, each offering what the answer above it makes available: the hosts in the Inventory, then the accounts that host's Bichon reports, then that account's Synced Folders — the set [reconcile-folders](/cli-reference/bichon/reconcile-folders/) writes. `--before` has no list to pick from, so it is a free-text prompt, validated as `YYYY-MM-DD` after the answer. Without a terminal, omitting it fails naming the flag.

Pickers cannot hang a script. With no terminal a lone candidate is implied, and several fail naming the flag rather than waiting for an answer nobody can give.

The folder picker offers Synced Folders and nothing else, and `--folder` accepts nothing else either. The Archive can vouch only for folders it ingests continuously; sidecars for a folder that has left the set linger in the append-only Archive, so a coverage question about one would be answered on stale evidence. A folder outside the set used to match no store message and report `covered` over nothing.

The set is the `sync_folders` Bichon holds: what [reconcile-folders](/cli-reference/bichon/reconcile-folders/) last wrote, and what the Archive is ingesting now. `examples/bichon-expunge.sh` starts from the same set and then narrows it — it re-reconciles against the live folder list and refuses a folder pending removal. That extra step guards a deletion. This command only reports, so it stops at the set.

An `--account` Bichon does not report is an error naming the accounts it does.

## Exit codes

| Code | Meaning                                                     |
| ---- | ----------------------------------------------------------- |
| 0    | covered — every store message in the window is archived     |
| 1    | gap — the JSON/human output names the missing messages      |
| 2    | usage error, unreachable host or API, or an unkeyed sidecar |

## Output

```json
{
  "host": "auberge",
  "account": "me@example.com",
  "folder": "INBOX",
  "before": "2026-05-13",
  "status": "covered",
  "store_internal_date_count": 388,
  "store_without_internal_date": [],
  "store_messages": 388,
  "matched": 388,
  "missing": [],
  "unverifiable": { "contested": [] }
}
```

`missing` entries carry the canonical `message_id` (what the sidecar would record, `sha256:<hex>` for a body with no `Message-ID`), the message date, and the store uid.

## Access

Needs both the Bichon API (account lookup, envelope search, and body download for messages with no `Message-ID` — same base URL resolution and `bichon_api_token` as [reconcile-folders](/cli-reference/bichon/reconcile-folders/)) and SSH to the host (the sidecar walk runs under `sudo`). A host whose Bichon reports no accounts is refused in the same words there, because all three commands reach the API through one place.

## Examples

```bash
# Pick the host, account and folder; only the cutoff has to be typed
auberge bichon verify-coverage --before 2026-05-13

# One folder, human verdict
auberge bichon verify-coverage --host auberge --account me@example.com --folder INBOX --before 2026-05-13

# Scriptable: gate 3 of bichon-expunge.sh consumes this
auberge bichon verify-coverage --host auberge --account me@example.com \
  --folder INBOX --before 2026-05-13 --output json
```

## When a gap is found

Run [`auberge bichon rescan --host <HOST> --account <EMAIL>`](/cli-reference/bichon/rescan/): a full pass archives everything the cursor skipped, and a re-run of this command then reports `covered`.
