# Stream recovery

How GATEWAY's Helius WebSocket manager (`solprint-gateway/src/stream/`) keeps
subscriptions alive and what it does, and does not, guarantee about continuity.
Streams are **off by default** (`GATEWAY_STREAMS_ENABLED=false`) and need
`HELIUS_API_KEY`.

**The core rule: continuity is never assumed.** Every interruption ends either in a
recovery (with the recovered events labelled `RECOVERY`), an explicit `GAP`, or an
explicit `CONTINUITY_UNKNOWN`.

## Connection state machine

```text
DISABLED ── (streams on, key present) ──▶ DISCONNECTED
DISCONNECTED ─▶ CONNECTING ─▶ SUBSCRIBING ─▶ CONNECTED ⇄ DEGRADED
      ▲                                         │
      └──────── RECONNECTING (backoff) ◀────────┘  error, close, missed pong
any state ─▶ BUDGET_EXHAUSTED  (budget no longer admits P2; re-checked every 60 s)
any state ─▶ SHUTTING_DOWN     (cancellation)
```

* `DEGRADED`: connected, but at least one subscription was rejected by the server.
* One manager task owns the socket; callers talk to it through a bounded command
  channel (`StreamHandle`). Subscriptions are capped at `HELIUS_WS_MAX_SUBSCRIPTIONS`.
* Each connection costs 1 credit and is budget-admitted at P2; received bytes are
  metered (see [API_BUDGET_POLICY.md](API_BUDGET_POLICY.md)).

## Subscriptions

| Kind | Notes | After a reconnect |
|---|---|---|
| `slot`, `root` | no commitment parameter | `CONTINUITY_UNKNOWN` |
| `logs` | exactly one `mentions` address, explicit commitment | **recovered** when it has a durable key and a checkpoint, otherwise `CONTINUITY_UNKNOWN` |
| `account` | explicit commitment | `CONTINUITY_UNKNOWN` (the next notification carries the current state) |
| `signature` | fires once at the requested commitment, then the server ends it; removed locally after it fires | resubscribed if it has not fired |
| `program` | must carry at least one filter (unfiltered program streams are refused) | `CONTINUITY_UNKNOWN` |

The server acknowledges each subscribe request with a server-side id. The manager maps
server ids to stable local `SubscriptionId`s per **connection generation**: every new
connection increments the generation, the map is rebuilt from the new acknowledgements,
and every event carries its generation. A notification for an id from an older
connection can therefore never be attributed to the wrong subscription. Unsubscribing
sends the matching unsubscribe request and drops the local entry.

## Liveness

* A ping is sent every 30 s; if no pong arrives within 10 s, the
  connection is treated as dead and replaced, even if TCP still looks open
  (`silent_connection_is_detected_by_heartbeat_and_replaced`).
* Connect attempts time out after 10 s.
* Reconnection uses exponential backoff from 1 s to 60 s with jitter (each wait is
  between half and all of the current step). A successful
  connection resets it. The number of reconnects and the last error are visible on the
  Gateway page.

## Recovery of `logs` subscriptions

A `logs` subscription with a durable key persists a checkpoint (last slot and signature)
to `solprint.gateway_stream_checkpoints`. After a reconnect (or a restart, when the
checkpoint exists):

1. The subscription is re-established first, so new live events are not missed while
   history is fetched. Live events are delivered as they arrive (they are **not**
   buffered behind the recovery).
2. GATEWAY calls `getSignaturesForAddress(mentions, until = checkpoint signature)` at
   P2, bounded to 3 pages of 100 signatures and 20 s.
3. Results are emitted **oldest first** as `Recovered` events (signature, slot, block
   time, failed flag) with provenance `RECOVERY` and the subscription's commitment.
4. Signatures already seen (live or recovered) are suppressed by a bounded dedup set of
   4 096 recent signatures; duplicates are counted.
5. The checkpoint advances to the newest recovered signature.
6. If the bound was reached before the checkpoint was found, or the request failed, a
   `GAP` event reports the slot range that may be missing, and the subscription's
   continuity becomes `GAP`. Otherwise it becomes `RECOVERED`.

Not guaranteed:

* Recovered events carry the signature-level facts above, not the original log lines.
  A consumer that needs details fetches the transaction through GATEWAY.
* Ordering between recovered and live events is by slot, not by arrival: consumers must
  order by `slot` and de-duplicate by signature.
* Events older than the recovery bound are not fetched; they are reported as a gap.

## Backpressure

Events go to one bounded channel (1 024 events). The socket reader never waits for a
slow consumer: when the channel is full the event is dropped and counted, and the next
event delivered for that subscription is preceded by a `CONTINUITY_UNKNOWN` marker, so
the loss is never silent.

## Commitment and forks

Each event carries the commitment its subscription used. `processed` and `confirmed`
data can be rolled back by a fork; only `finalized` data is final. GATEWAY does not
reconcile forks: consumers that act on non-finalized data must expect reversals.

## Shutdown

On cancellation the manager stops reconnecting, leaves the socket loop and ends; the
event channel closes so consumers see the end of the stream. Checkpoints changed since
the last flush are written by GATEWAY's final flush.
