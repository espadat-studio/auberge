# ADR-0088: Expunge Coverage is per-account identity, bounded by the store

## Status

Accepted, 2026-11-12. **Amends ADR-0012** §83 (first-sight `folder` stays, but stops scoping coverage) and **ADR-0013** (the contested-id rule and the count gate).

## Decision

**Expunge Coverage** proves the Email Archive holds every message an expunge would delete. It is two claims, each about one pair of things counted the same way:

1. **Upstream ⊆ Internal Store, by count.** For the folder and window, the IMAP message count must not exceed the Internal Store's message count. Both are raw counts of messages, and both use the IMAP internal date: IMAP `SEARCH BEFORE` on one side, the store's `internal_date` on the other. `auberge bichon verify-coverage` reports the store count in its verdict, and `examples/bichon-expunge.sh` compares it to its IMAP count.
2. **Internal Store ⊆ Email Archive, by identity, per account.** A store message in the folder's window is covered when an archived body with its identity exists **anywhere in the account's archive**. The sidecar's `folder` is restore metadata. It no longer scopes the match.

Contested identities are counted per account, with no window. For a Message-ID claimed by a store message in the folder's window, every store message in the account claiming it is compared against every archived body in the account keyed by it. Fewer bodies than claimants is a gap.

The archive count, meaning distinct archived Message-IDs scoped to the folder, is no longer compared to anything.

## Why

A sweep on 2026-11-12, after a full rescan with `total_failures=0`, still refused 6 folders. Each refusal traced to a coverage gate measuring the wrong thing, not to an unarchived message.

**Moved messages fail a folder-scoped identity match.** ADR-0012:83 records `folder` at first sight and accepted the cost because "moves are rare". It also counted on a moved message being "re-archived under a second envelope id". ADR-0013's amendment removed that second copy: a byte-identical body is deduped unpublished. A message moved after archiving therefore has one sidecar naming its old folder, and its new folder can never claim it. Measured against the store for the window before 2026-08-14:

| Account, folder    | Store | Sidecar in this folder | Sidecar in another folder                       | No sidecar |
| ------------------ | ----- | ---------------------- | ----------------------------------------------- | ---------- |
| `x@riou.one` INBOX | 1268  | 1245                   | 23 (`Archive.2024/2025/2026`)                   | 0          |
| `g@riou.one` INBOX | 302   | 285                    | 16 (`Archive`, `Archives.*`, `Trash`, `Drafts`) | 1          |

Every one of the 39 "unarchived" messages was archived. Only the 1 with no sidecar is a real gap.

Recording every folder a body was seen in was rejected. It rewrites sidecars ADR-0012 made write-once, it learns of a move only when a full pass happens to re-list the message, and a message moved into a folder that is not synced would still fail. The question an expunge asks is whether the archive holds the message. Where the message was first seen does not change that answer.

**The count gate compared distinct ids to raw messages.** It counted distinct archived Message-IDs in the folder against the IMAP message count. A folder holding one Message-ID twice can never pass:

| Account, folder            | IMAP | Store | Distinct ids in store | Archive count reported |
| -------------------------- | ---- | ----- | --------------------- | ---------------------- |
| `dev@sripwoud.xyz` Archive | 442  | 441   | 432                   | 432                    |
| `me@sripwoud.xyz` Archive  | 294  | 294   | 287                   | 287                    |

Counting archived bodies instead does not fix it, because the archive dedups byte-identical bodies too. The gate's remaining job since #400 is the one identity cannot do: prove the live mailbox holds nothing the store never ingested. That is a comparison of IMAP against the store, both raw. Against that rule `me@` Archive passes, and `dev@` Archive reports the 1 message the store never ingested.

**Why contested ids widen with the match.** A self-sent mail puts one Message-ID in Sent and in INBOX, with different bytes (`Received:` headers). If only the Sent copy is archived, counting INBOX's claimants alone against the account's bodies reads 1 ≤ 1 and calls INBOX covered when its body is missing. Claimants and bodies have to be counted over the same scope, and the other claimant can sit outside the window.

## Consequences

- A restore places a moved message in its first-sight folder. Unchanged from ADR-0012:83.
- The identity match reads the store for the whole account, not only the window, to count claimants. That is one more paged `search-messages` walk per account, the same endpoint the archive pass already reads.
- `verify-coverage`'s JSON verdict gains the folder's store count. The script stops reading the sidecars for a count. It still reads them for identity, through `verify-coverage`.
- A gap now names one of two things: a message the store never ingested (count), or a store message no archived body vouches for (identity). Both are real.

## References

- ADR-0012 §83: first-sight folder, kept as metadata.
- ADR-0013: Message-ID identity and the contested-id count bound, both widened to the account here.
- ADR-0007: expunge stays an external script; this changes what it proves, not who runs it.
- #400: identity coverage against the store, and why the count gate was kept.
