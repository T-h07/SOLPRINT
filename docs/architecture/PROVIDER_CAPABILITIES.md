# Provider capabilities

Capability matrix for the external providers that GATEWAY (Phase 01) talks to.
Every row was checked against the provider's official documentation on
**2026-11-26**. Unless a row says **request-verified**, it is
**documentation only**: nothing has been confirmed by a live request. The
owner's optional live diagnostics ([GATEWAY.md](GATEWAY.md#diagnostics)) are
how a row moves to request-verified.

GATEWAY enforces the limits below in configuration. Defaults are deliberately
lower than the documented ceilings. If a provider changes its terms, update this
file, the defaults in [CONFIGURATION.md](../development/CONFIGURATION.md) and the
cost registry together.

## Sources

| Provider     | Official source                                                                                                    | Notes                                                                 |
| ------------ | ------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| Helius       | helius.dev/docs (billing/credits, billing/rate-limits, websockets, common error codes); llms.txt dated 2026-11-22   | Free plan only                                                        |
| Jupiter      | developers.jup.ag/docs (rate limits, Price V3, Tokens V2, Swap V2 order)                                           | Free tier with key; keyless tier documented as a fallback             |
| DEX Screener | docs.dexscreener.com/api/reference and its linked OpenAPI 3.0.3 spec; API terms dated 2023-08-18                   | No API key                                                            |
| Solana       | `solana-foundation/solana-com` repository, `apps/docs/content/docs/en` (the source of solana.com/docs), 2026-11-24 | solana.com answered HTTP 429 on 2026-11-26, so the source repository was read instead |

## Helius (Free plan)

| Capability                                                                                                                                                                 | Endpoint / auth                                            | Rate group   | Credits                          | Batch max          | Status in GATEWAY                          |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ------------ | -------------------------------- | ------------------ | ------------------------------------------ |
| Standard JSON-RPC: getHealth, getVersion, getSlot, getBalance, getAccountInfo, getMultipleAccounts, getTokenAccountBalance, getTokenAccountsByOwner, getTokenSupply, getTokenLargestAccounts | `https://mainnet.helius-rpc.com/?api-key=…` (query parameter only) | RPC 10 req/s | 1 each                           | undocumented (count every item) | Enabled                                    |
| getBlockTime, getSignaturesForAddress, getTransaction (historical)                                                                                                       | same                                                       | RPC 10 req/s | 1 each                           | getTransaction 100; other historical methods 10 | Enabled; getSignaturesForAddress is bounded |
| getSignatureStatuses (Phase 02 finality reconciler; added 2026-11-27, Helius lists it under historical data) | same | not stated on the credits page (treated as RPC) | 1 | 256 signatures (Solana docs) | Enabled; `searchTransactionHistory: false`; a `null` entry means unknown, not failed |
| getBlock                                                                                                                                                                   | same                                                       | RPC 10 req/s | 1                                | 10                 | Disabled by default (`HELIUS_GET_BLOCK_ENABLED`); budget-gated, never polled |
| getProgramAccounts                                                                                                                                                         | same                                                       | 5 req/s      | 10                               | —                  | **Not implemented**                        |
| DAS / Enhanced Transactions / getTransactionsForAddress                                                                                                                    | same                                                       | 2 req/s      | 10 / 100 / 10 per 100 tx         | —                  | **Not implemented**                        |
| sendTransaction                                                                                                                                                            | same                                                       | 1 req/s      | —                                | —                  | **Prohibited** (non-executing phase)       |
| Standard WebSocket: slot, logs, account, signature, program, root subscribe                                                                                               | `wss://mainnet.helius-rpc.com/?api-key=…`                  | 5 connections, 1000 subscriptions each | 1 per connection open + 2 per 0.1 MB uncompressed (20/MB) | — | Enabled, one connection by default |
| transactionSubscribe, LaserStream, enhanced WebSockets                                                                                                                     | —                                                          | —            | —                                | —                  | **Not available on Free**; disabled        |
| Admin usage API (`admin-api.helius.xyz/v0/admin/projects/{id}/usage`)                                                                                                     | requires project id                                        | —            | —                                | —                  | Free availability undocumented; **not used** (ACCOUNT_USAGE_UNKNOWN) |

- **Plan:** 1,000,000 credits per month. Free has no autoscaling. Additional credits cost $5 per million on paid plans. GATEWAY never enables overage.
- **Rate-limit window:** "Rate limits reset every minute". A 429 can come as HTTP 429 or as JSON-RPC `{"code":-32005,"message":"Too many requests"}`.
- **Retry policy:** retry on 408, 429 and 5xx; never retry 400/401/403/404/409/422.
- **WebSocket liveness:** the server has a 10-minute inactivity timeout, and the docs say to ping at least once per minute.
- **Unresolved documentation conflict:** staked-connection availability on Free. It is irrelevant here because GATEWAY never sends transactions.

## Jupiter (Free tier with API key)

| Capability                       | Endpoint                                                          | Rate group                       | Batch max        | Status in GATEWAY                              |
| -------------------------------- | ----------------------------------------------------------------- | -------------------------------- | ---------------- | ---------------------------------------------- |
| Price V3                         | `GET https://api.jup.ag/price/v3?ids=` header `x-api-key`         | shared general bucket            | 50 ids           | Enabled                                        |
| Tokens V2 search                 | `GET https://api.jup.ag/tokens/v2/search?query=`                  | shared general bucket            | 100 mints        | Enabled                                        |
| Tokens V2 category               | `GET /tokens/v2/{toporganicscore\|toptraded\|toptrending}/{5m\|1h\|6h\|24h}?limit=` | shared general bucket | `limit` 1–100 | Enabled for RADAR market discovery (Phase 03); only runs when `RADAR_AUTO_DISCOVERY_ENABLED=true`; cached 60 s fresh / 300 s stale |
| Tokens V2 recent                 | `GET /tokens/v2/recent` (mints whose first pool was created recently) | shared general bucket       | —                | Enabled for RADAR market discovery (Phase 03); same opt-in and cache |
| Tokens V2 tag                    | `GET /tokens/v2/tag`                                              | shared general bucket            | —                | **Not implemented** (no approved use)          |
| Swap V2 order, quote only        | `GET https://api.jup.ag/swap/v2/order?inputMint&outputMint&amount[&slippageBps]` with **no** `taker` | shared general bucket | — | Enabled; GATEWAY rejects any response that carries a transaction |
| Swap V2 execute / build; tx submit | `POST /swap/v2/execute`, `/swap/v2/build`, `tx.jup.ag`, `/tx/v1/submit` | separate             | —                | **Prohibited**; paths are blocklisted in the transport |
| lite-api, Ultra, Swap V1 `/quote` | —                                                                | —                                | —                | Deprecated; not used                           |

- **Limits:** with a key, 1 req/s and 60 req/min. Without a key, 0.5 req/s and 30 req/min. The window is a 60-second sliding window, counted per organisation.
- **Discovery listings (Phase 03):** category and recent listings are the token rows Tokens V2 search returns, with optional per-window stats. Jupiter documents that generic tokens such as SOL and USDC are filtered out. GATEWAY parses counts only as exact non-negative integers and leaves anything else unknown. They cost no Helius credits but share the Jupiter bucket, so one RADAR cycle sends at most one request per listing, and the cache coalesces repeated cycles. Operations: `jupiter.tokens.category`, `jupiter.tokens.recent`.
- **Shared bucket:** price, token and swap requests share one bucket. GATEWAY therefore keeps one `jupiter.general` limiter for all three.
- **Rate-limit headers:** `x-ratelimit-remaining`, `x-ratelimit-current`, `x-ratelimit-reset` (unix seconds), sent on 200 and 429 responses. There is no lockout after a 429.
- **Price V3 response:**
  - a map keyed by mint, with `usdPrice` (JSON number), `blockId` (integer or null), `decimals`, `priceChange24h` (number or null), `createdAt` and `liquidity`;
  - unpriced mints are omitted, and GATEWAY reports them as not found;
  - JSON numbers are converted to `Decimal` through their exact textual form, never through `f64` arithmetic.
- **Order response:**
  - `inAmount`, `outAmount` and `otherAmountThreshold` are strings;
  - `priceImpact` is in percentage points; `priceImpactPct` is deprecated;
  - a 400 carries `{requestId, error}`;
  - the exact no-route behaviour is undocumented. GATEWAY maps a 400 with `error`, or a 200 without `outAmount`, to `NoRoute`.

## DEX Screener (no key)

| Capability                    | Endpoint                                          | Rate group | Batch max     | Status in GATEWAY                   |
| ----------------------------- | ------------------------------------------------- | ---------- | ------------- | ----------------------------------- |
| Pairs by token addresses      | `GET /tokens/v1/solana/{a,b,…}` (bare `Pair[]`)   | 300/min    | 30 addresses  | Enabled                             |
| Pools of one token            | `GET /token-pairs/v1/solana/{token}` (bare `Pair[]`) | 300/min | 1             | Enabled                             |
| Pair by address               | `GET /latest/dex/pairs/solana/{pair}` (`{schemaVersion, pairs|null}`) | 300/min | 1 | Enabled                     |
| Search                        | `GET /latest/dex/search?q=` (`{schemaVersion, pairs}`, cross-chain) | 300/min | — | Enabled; non-Solana pairs are dropped |
| Token profiles / boosts       | `/token-profiles/latest/v1`, `/token-boosts/latest/v1`, `/token-boosts/top/v1` | 60/min | — | Implemented, **disabled by default**; on-demand only, never scanned |
| Orders                        | `/orders/v1/{chainId}/{token}`                    | 60/min     | —             | **Not implemented**                 |

- **Base URL:** `https://api.dexscreener.com`.
- **Prices:** `priceUsd` and `priceNative` are strings and are parsed as `Decimal`. Volume, liquidity, FDV and market cap are JSON numbers and are converted through their exact text.
- **Timestamps:** responses carry **no snapshot timestamp**. Freshness is the GATEWAY observation time, and source age is reported as `UNKNOWN_SOURCE_AGE`. `pairCreatedAt` is pair creation time (ms), not data time.
- **Terms:**
  - the licence is revocable;
  - no competing product, no resale, and no making data available to third parties;
  - SOLPRINT is self-hosted and single-owner, and does not redistribute data.

## Solana public RPC (fallback only)

| Capability                | Endpoint                          | Limits                                                                  | Status in GATEWAY |
| ------------------------- | --------------------------------- | ----------------------------------------------------------------------- | ----------------- |
| Standard JSON-RPC (read)  | `https://api.mainnet.solana.com`  | 100 req / 10 s per IP; 40 req / 10 s per IP for a single method; 40 concurrent connections; 40 connections / 10 s; 100 MB / 30 s | Fallback only; 1 in-flight request; conservative default of 2 req/s |
| WebSocket                 | `wss://api.mainnet.solana.com`    | as above                                                                | **Not used**      |

The docs call this endpoint "not intended for production applications". GATEWAY
uses it only when **all** of the following hold:

- the caller allowed fallback;
- the method has equivalent semantics;
- the deadline still allows the request;
- the result meets the caller's freshness requirement;
- Helius is unavailable (circuit open or budget exhausted).

## JSON-RPC semantics relied on (Solana docs)

- **getTransaction:**
  - returns `null` when the transaction is not found or not yet confirmed;
  - rejects `processed` commitment;
  - needs `maxSupportedTransactionVersion` (0 or 1) for versioned transactions. GATEWAY always sends `0`, the highest version Phase 02 decoding is scoped for, and keeps the raw JSON as evidence. An unsupported-version error is surfaced as a typed error rather than retried.
- **getSignaturesForAddress:**
  - `limit` is 1–1000, results are newest first, and `before`/`until` paginate;
  - GATEWAY caps pages and total items per call (see [STREAM_RECOVERY.md](STREAM_RECOVERY.md)).
- **getMultipleAccounts:** at most 100 keys per call.
- **WebSocket subscriptions:**
  - the default commitment is `finalized`, and GATEWAY always sends commitment explicitly;
  - `logsSubscribe` `mentions` accepts exactly one address;
  - `signatureSubscribe` delivers one terminal notification and then cancels itself.

## Credentials

Helius and Jupiter keys are read from `.env` into `Secret<T>`. They are never
logged, returned by the API, or exposed to the frontend. Helius accepts its key
only as a URL query parameter. GATEWAY therefore redacts every Helius URL to its
origin before it reaches logs, errors or metrics.

Provider credentials were exposed in an earlier conversation, and their rotation
has not been independently confirmed. **Rotating them is an owner
responsibility.** See [SECURITY_MODEL.md](SECURITY_MODEL.md).
