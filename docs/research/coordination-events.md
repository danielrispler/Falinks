# Durable coordination events and live notifications

Research for [Research durable coordination events and live agent notifications](https://github.com/danielrispler/Falinks/issues/6), 2026-09-29. Recommendation for discussion, not an architecture decision. No implementation or runtime behavior tests were performed.

## Scope and agreed requirements

One machine, one repository, a Rust engine and two clients. Events survive engine restart and disconnected agents can replay them. Keep the complete experiment event history until explicit reset. Planned changes notify without reserving symbols; contextual replies support voluntary bounded waiting. Published changes can invalidate proposals. Notifications never refresh snapshots silently or replace final publication validation. The agent runtime and its interruption mechanism remain unchosen.

## Recommendation

Use an embedded SQLite database as the durable coordination record, one engine-owned writer, and an ordered event sequence. Keep pub/sub in memory only as a wake-up mechanism: replayable records remain on disk. Prefer HTTP commands plus server-sent events (SSE) if the selected runtime can consume an HTTP stream; otherwise a local framed stream can carry the same records. Do not introduce Kafka or a graph database for two local clients.

This is an engineering inference from the documented capabilities below, not a measured latency or effort claim. The publication protocol must settle the cross-store crash boundary before implementation.

## Storage comparison: verified capabilities and implications

| Option | Verified capability | Assessment for this experiment |
| --- | --- | --- |
| SQLite | Transactions provide atomic database changes; rollback journals recover interrupted transactions. [Atomic commit](https://www.sqlite.org/atomiccommit.html) | Best starting candidate: append events, update proposal status, and deduplicate requests in one database transaction. |
| Append-only file | Rust `File::sync_all` attempts to synchronize content and metadata; dropping a file is not equivalent to successful synchronization. [Rust File](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all) | Viable, but framing, partial-tail recovery, indexes, state replay, acknowledgements, and write-error policy become application work. A newline alone is not a transaction protocol. |
| Kafka | Persistent retained events can be reread; ordering is per partition; the system uses servers and clients. [Kafka introduction](https://kafka.apache.org/43/getting-started/introduction/) | Useful analogy, unnecessary service/deployment surface here. More partitions do not automatically give one global order. |

SQLite WAL allows readers alongside a writer, but still only one writer at a time, and requires same-host access. This fits the scope; a serialized single engine may also work with the default rollback journal. WAL is an option, not a correctness requirement. Avoid holding database read transactions open for the lifetime of a subscriber. [SQLite WAL](https://www.sqlite.org/wal.html)

For a WAL configuration, `synchronous=FULL` synchronizes the WAL on each commit; `NORMAL` preserves consistency but can lose committed transactions after power loss. Engine-process crash and machine power loss are different guarantees. Verify effective settings and filesystem assumptions; on macOS, inspect SQLite's `fullfsync` setting if promising power-loss durability. Hardware/storage failure is not solved by these settings. [SQLite pragmas](https://www.sqlite.org/pragma.html#pragma_synchronous)

Use database-assigned monotonic event IDs, not timestamps, for order. SQLite `AUTOINCREMENT` prevents reuse of committed row IDs after deletion, with overhead; IDs need not be gapless. An experiment ID paired with the sequence makes explicit reset distinguishable from replay. [SQLite autoincrement](https://www.sqlite.org/autoinc.html)

## Transport comparison

| Option | Verified capability | Assessment |
| --- | --- | --- |
| SSE plus HTTP commands | SSE defines event types, IDs, reconnection and `Last-Event-ID`. [HTML standard](https://html.spec.whatwg.org/multipage/server-sent-events.html) | Good fit for engine-to-client notifications; send replies/plans through ordinary requests. Non-browser clients must implement or reuse reconnection support. |
| WebSocket | Bidirectional communication over a connection. [RFC 6455](https://www.rfc-editor.org/info/rfc6455/) | Valid if already native to the integration. Still needs our persistence, cursor, replay and deduplication protocol; it does not eliminate that work. |
| Local stream | Rust provides Unix stream sockets with read/write and timeout support on Unix. [Rust UnixStream](https://doc.rust-lang.org/std/os/unix/net/struct.UnixStream.html) | Small local option when both client adapters are controlled. Define record framing, maximum size and reconnect behavior; not a portable browser interface. |

Transport delivery does not imply the LLM reads a message immediately. The selected runtime must expose a safe interruption or message-drain point. Measure event commit-to-delivery and delivery-to-agent-reaction separately. No millisecond latency promise follows from choosing memory, SQLite, or SSE.

## Proposed event semantics

These are design proposals, not documentation facts:

- `planned`: contextual intent, affected symbols, proposal, base snapshot, optional estimated completion. Advisory and supersedable.
- `published`: engine-authored accepted snapshot transition, actual affected symbols, diff reference, originating proposal and author's explanation.
- `message`: directed reply linked to a proposal or earlier event; explanation is agent-authored evidence, not engine-verified truth.
- `cancelled` or `superseded`: close/update earlier intent so reconnecting agents do not mistake abandoned work for current activity. Exact vocabulary belongs in the protocol ticket.

Common envelope: experiment ID, event ID, kind, author, proposal ID, relevant symbols, payload and optional reply-to ID. Timestamp helps humans but does not establish order. Snapshot and diff references must remain retrievable for the retained history. Validate payload structure, sizes, identity and referenced objects at ingestion.

Derive subscriptions from edited symbols and declared dependencies; explicit file watches may cover non-symbol changes once their rules are specified. Replies also route to their recipient. Publication updates stale status independently of notification delivery. An agent cannot clear staleness by acknowledging a message.

Persist bounded-wait context and deadline if waits are expected to survive restart. At restart, reevaluate the relevant condition and expired deadline; do not restart an unlimited wait. Plans and deadlines are advisory, and no reservation is acquired.

## Commit, replay and duplicates

Proposed minimal correctness rules:

1. Serialize accepted coordination commands. In one SQLite transaction append the event and update associated coordination state. Commit before acknowledging success or notifying subscribers. A failed commit means no success acknowledgement.
2. Require a client-generated request ID with a uniqueness constraint. A client retry after losing the response receives the earlier outcome rather than creating a second plan/reply/publication.
3. Treat live notification as a hint to query durable records after the cursor, in order. Register the wake-up receiver before querying the backlog, then drain durable rows; use a retained wake-up/generation mechanism or a final synchronized recheck before sleeping. A naive “read backlog, then subscribe” loses events in the gap.
4. Read replay in short batches, closing each database read transaction. Slow clients must not block publication. If a transient queue overflows, recover from the durable cursor. Tokio broadcast explicitly reports lag when old retained values are discarded; that channel is not durable storage. [Tokio broadcast](https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html)
5. Persist a processed cursor only after handling the corresponding records. Reconnection can deliver duplicates; handlers need event-ID deduplication and idempotent actions. SSE's last-received ID is not proof the application completed processing.
6. Filtered streams must advance a scan cursor across irrelevant records, with clear subscription-version semantics. A changed dependency/subscription should revalidate against current state and choose a new replay boundary; replaying only under today's filter can miss previously relevant messages.
7. On explicit reset change the experiment ID and reject old cursors clearly. While full history is retained, no time-based expiry is needed; disk-full is a visible failed write, not permission to silently delete history.

A crash after commit but before notification is safe for replay: restart/disconnect handling reads committed events. A crash before commit must not leave a published-looking event visible. This statement applies to database-owned state only; repository publication needs the next section.

## Critical boundary: repository publication is not a SQLite transaction

A transaction cannot atomically combine a SQLite commit with arbitrary Git ref changes or filesystem writes. Git `update-ref` can condition a ref update on its expected old value and transact ref updates, but does not enlist SQLite or make a multi-file checkout atomic. [Git update-ref](https://git-scm.com/docs/git-update-ref)

Therefore neither of these sequences alone is safe:

- Publish files/ref, then append event: crash can leave accepted code without its coordination record.
- Append `published`, then publish files/ref: crash can leave a success event for code that was never published.

Two designs deserve consideration in the protocol ticket:

**A. Database-owned published snapshot pointer.** Prepare immutable snapshot contents durably first. In a database transaction compare the expected published snapshot, switch the pointer, update proposal states, and append `published`. Clients obtain engine snapshots through this pointer. Filesystem checkout/Git mirror is a recoverable projection and is not the publication authority. Orphaned prepared snapshots are harmless, but required contents must survive restart and remain retained. This can avoid dual-write ambiguity at the authority boundary; it changes the meaning of publication and must be explicitly accepted.

**B. Git-owned published ref with a recovery journal.** Before touching the ref, durably record a pending publication with a unique operation ID, old/new snapshot IDs and complete event context. Prepare the immutable commit, use compare-and-swap ref publication, then finalize status and append the unique `published` event. On startup block normal publication, inspect pending records and the authoritative ref: finalize if it matches the intended new state, safely abort/retry if still at the expected old state, and pause for explicit reconciliation if unexpected. Serializing publication and stopping further publication after a finalization error are necessary to keep recovery interpretable. Git object/ref durability settings also need specification if the guarantee includes power failure. This is recovery, not cross-store atomicity; define what reads are visible while a publication is pending.

If the ordinary mutable checkout is itself the authority, neither option automatically solves partial multi-file application. That workflow needs its own staging/recovery protocol and external-writer boundary. Do not present an SQLite choice as resolving it.

## Decisions still needed

- Publication authority and restart recovery boundary (A, B, or another explicitly specified mechanism).
- First runtime adapter and supported safe interruption point; transport follows its capabilities.
- Guarantee scope: engine restart is agreed; power-loss behavior needs explicit language and platform validation.
- Cursor/subscription update semantics, bounded-wait defaults and exact event schema.

Recommended future checks, not tests performed: crash before/after event commit and publication; replay during concurrent publication; duplicate command retry; slow subscriber reconnect; restart with pending publication; changed subscription; explicit reset with old cursor; disk-full rejection. These should be scenario checks against the eventual protocol, not a broad test framework.
