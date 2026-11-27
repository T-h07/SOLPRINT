# Module boundaries

## Crate dependency graph (enforced by Cargo)

| Crate | Depends on (workspace) |
|---|---|
| `solprint-core` | — |
| `solprint-config` | core |
| `solprint-observability` | config |
| `solprint-storage` | core, config |
| `solprint-gateway` | core, config, storage |
| `solprint-api` | core, config, observability, storage, gateway |
| `solprint-app` | api, config, core, gateway, observability, storage |

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
7. **Secrets stay in `solprint-config`'s `Secret<T>`** until the exact point of use. No
   crate logs a `Secret` (its `Debug` is redacted and it has no `Display`).

## Future modules

Each roadmap module becomes its own crate under `backend/crates/` when its phase starts,
depending on `core` (and `storage` if it persists). A module never calls another module's
internals; cross-module communication will go through explicit interfaces defined when
the first two modules need to talk (expected: DATA ENGINE → RADAR). Execution-capable code
(SNIPER) is additionally gated by the `bans` list in `backend/deny.toml`: signing and
wallet crates cannot enter the graph without an explicit, reviewed change to that list.

## Frontend boundaries

* `components/ui` has no knowledge of the API, routes or features.
* `features/*` may use `lib/api` and `components/ui`; pages compose features.
* `fixtures/` is imported only by the design-system showcase (unit-tested).
* `types/generated` is written by the backend build; never edited by hand.
