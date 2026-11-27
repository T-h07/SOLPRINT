# API budget policy

How GATEWAY spends the Helius credit allowance (`budget.rs`). Jupiter and DEX Screener
are not credit-metered; they are bounded by their rate limits and their requests are
counted in usage. The public Solana RPC is free and rate-limited.

**Paid features stay off.** SOLPRINT never enables overage billing, automatic top-ups,
paid plans or paid endpoints. If the allowance runs out, traffic stops; it is never
silently bought.

## Allowance and reserve

| Setting | Default | Meaning |
|---|---|---|
| `HELIUS_MONTHLY_CREDITS` | 1 000 000 | the plan's credits per billing cycle (Free plan) |
| `HELIUS_RESERVE_PERCENT` | 20 | share protected for P0–P1 (here 200 000) |
| `HELIUS_BILLING_CYCLE_START_DAY` | 1 | day of month (1–28, UTC) the cycle resets |

*Normal budget* = allowance − reserve (800 000 by default). Lower priorities can only
spend the normal budget; the reserve exists so that safety-relevant reads (P0–P1) still
work when routine traffic has used everything else.

## Cost registry

Every Helius JSON-RPC method GATEWAY sends has a registered cost
(`operations::helius_credits`); a method without one is **never** sent. All 14 standard
methods cost 1 credit per call. A batched request is charged per item, because Helius does
not document batch accounting. WebSocket usage is metered as documented: 1 credit per
connection opened plus 2 credits per 0.1 MB of uncompressed messages received (20 per
MB, accumulated in bytes and charged as whole credits).

## Budget states

States are classified from the persisted cycle usage. Each state admits a range of
priorities (P0 is the most important).

| State | Entered at (default) | Admits |
|---|---|---|
| `NORMAL` | below 70 % of the normal budget (< 560 000) | P0–P5 |
| `CONSERVATION` | ≥ 70 % of the normal budget (560 000) | P0–P3 |
| `CRITICAL` | ≥ 90 % of the normal budget (720 000) | P0–P2 |
| `NORMAL_BUDGET_EXHAUSTED` | normal budget used (800 000) | P0–P1, from the reserve |
| `HARD_LIMIT_REACHED` | allowance used (1 000 000) | nothing |
| `ACCOUNT_USAGE_UNKNOWN` | ledger not loaded (database unavailable at start) | P0–P2 |

Priorities: P0 safety-critical, P1 position monitoring, P2 stream continuity and gap
recovery, P3 interactive owner requests, P4 background enrichment, P5 discovery and
diagnostics.

`ACCOUNT_USAGE_UNKNOWN` is deliberately conservative: without the stored total, GATEWAY
cannot know how much of the cycle was already spent, so it behaves like `CRITICAL`
until the ledger loads.

## Admission

* Credits are **reserved atomically before** a request is sent (one compare-and-swap on
  the cycle total). Concurrent callers therefore cannot together exceed what the state
  admits: the reservation that would cross a limit fails with `BUDGET_DENIED`.
* A request that never reached the provider (refused locally, cancelled before
  dispatch) is refunded. A request that was sent is charged, even if it failed, because
  the provider may have charged for it.
* Retries are admitted individually; a retry that the budget refuses is not sent.
* The stream manager re-checks the budget (P2) before each connection and every 60 s; in
  states that do not admit P2 it disconnects and reports `BUDGET_EXHAUSTED`.

## Persistence and restarts

* Usage is accumulated in memory and flushed additively to
  `solprint.gateway_billing_cycles` every `GATEWAY_FLUSH_INTERVAL_SECS` (30 s) and at
  shutdown. On start the cycle total is loaded before normal traffic is admitted.
* At most one flush interval of usage can be lost in a crash. The dashboard shows the
  number of pending (unflushed) credits and the time of the last successful write.
* Cycles roll over on the configured start day; each cycle is its own row.

## What the ledger is, and is not

The ledger counts what **this SOLPRINT installation** sent, using the documented costs. It
is an estimate of Helius's own count, not a copy of it. Usage from other applications on
the same key, or differences in how Helius meters, are invisible to it. The UI therefore
labels the figures as SOLPRINT's local accounting and shows the projection as a linear
estimate. Helius's dashboard remains the authority for the bill.

## Visibility

`GET /api/v1/gateway/budgets` (and the Budget pane on the Gateway page) shows: state,
credits used and remaining, the reserve and every threshold, which priorities are
admitted, denied requests per priority, stream credits and bytes, cycle start and end, a
linear end-of-cycle projection, and ledger persistence status.
