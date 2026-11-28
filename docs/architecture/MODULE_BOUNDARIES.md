# Module boundaries

## Crate dependency graph (enforced by Cargo)

| Crate | Depends on (workspace) |
|---|---|
| `solprint-core` | — |
| `solprint-config` | core |
| `solprint-observability` | config |
| `solprint-storage` | core, config |
| `solprint-gateway` | core, config, storage |
| `solprint-data-engine` | core, config, storage, gateway |
| `solprint-radar` | core, config, storage, gateway, data-engine |
| `solprint-api` | core, config, observability, storage, gateway, data-engine, radar |
| `solprint-app` | api, config, core, gateway, data-engine, radar, observability, storage |

Rules:

1. **`solprint-core` has no I/O and no workspace dependencies.** It must stay usable from
   any future module (RADAR, FILTER, CORTEX, GUARD, REPLAY, SANDBOX, SNIPER) and from
   tests without a runtime.
2. **Dependencies point inward** (towards `core`). A lower crate never depends on a higher
   one; cycles are impossible by construction and must stay so.
3. **Only `solprint-app` composes.** It is the one place that wires configuration,
   logging, storage and HTTP together. Other crates take their dependencies as arguments.
4. **Only `solprint-storage` talks SQL.** Other crates call typed storage functions.
5. **Only `solprint-api` defines wire contracts.** Domain types are not serialised
   directly to clients; contracts are separate, versioned (`/api/v1`) and generated to
   TypeScript.
6. **Only `solprint-gateway` talks to external providers.** No other crate depends on an
   HTTP or WebSocket client for provider access. Provider JSON is decoded inside GATEWAY;
   other crates see only its normalized models and the traits in `traits.rs`. GATEWAY
   persists through the `GatewayStore` trait, implemented for `solprint_storage::Database`
   (and an in-memory store for tests); the SQL stays in `solprint-storage`.
7. **`solprint-data-engine` reaches providers only through GATEWAY.** It takes a
   `ChainSource` (implemented over `solprint_gateway::Gateway`; an in-memory source in
   tests), never an HTTP client, and it decodes only the normalized transaction JSON that
   GATEWAY returns. It persists only through `solprint_storage::Database`'s `de_*`
   functions. The API reads Data Engine state from storage; it never triggers ingestion
   or provider traffic.
8. **`solprint-radar` reads chain data only through the DATA ENGINE.** It consumes
   DATA ENGINE outbox events (consumer `radar`) and normalized records, and requests
   history as bounded DATA ENGINE backfills. Discovery listings come through GATEWAY's
   typed traits. It never decodes transactions, never holds an HTTP client and persists
   only through `solprint_storage::Database`'s `radar_*` functions. Later phases read
   RADAR through `solprint_radar::query` and its outbox events, never its tables
   ([RADAR.md](RADAR.md)).
9. **Secrets stay in `solprint-config`'s `Secret<T>`** until the exact point of use. No
   crate logs a `Secret` (its `Debug` is redacted and it has no `Display`).

## Future modules

Each roadmap module becomes its own crate under `backend/crates/` when its phase starts,
depending on `core` (and `storage` if it persists). A module never calls another module's
internals; cross-module communication will go through explicit interfaces defined when
the first two modules need to talk. The DATA ENGINE already publishes versioned events
through its transactional outbox (`de_outbox`, see
[DATA_ENGINE.md](DATA_ENGINE.md)); future consumers (RADAR, wallet intelligence) read
those events and the normalized records, never decoder internals or provider DTOs. Execution-capable code
(SNIPER) is additionally gated by the `bans` list in `backend/deny.toml`: signing and
wallet crates cannot enter the graph without an explicit, reviewed change to that list.

## Frontend boundaries

* `components/ui` has no knowledge of the API, routes or features.
* `features/*` may use `lib/api` and `components/ui`; pages compose features.
* `fixtures/` is imported only by the design-system showcase (unit-tested).
* `types/generated` is written by the backend build; never edited by hand.
