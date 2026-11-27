-- GATEWAY persistence (Phase 01).
--
-- Local accounting and observability state for the external-provider access
-- layer. Nothing here is provider data: it records what SOLPRINT itself spent,
-- measured and checkpointed, so budgets and stream continuity survive restarts.
--
-- Append-only: never edit this file after it has been applied anywhere.

-- One row per provider billing cycle. `credits_used` is SOLPRINT's own
-- conservative ledger (reservations are charged when a request is
-- dispatched), not the provider's authoritative account usage.
CREATE TABLE solprint.gateway_billing_cycles (
    provider        TEXT        NOT NULL CHECK (provider ~ '^[a-z_]{1,32}$'),
    cycle_start     TIMESTAMPTZ NOT NULL,
    cycle_end       TIMESTAMPTZ NOT NULL,
    credit_limit    BIGINT      NOT NULL CHECK (credit_limit >= 0),
    reserve_credits BIGINT      NOT NULL CHECK (reserve_credits >= 0 AND reserve_credits <= credit_limit),
    credits_used    BIGINT      NOT NULL DEFAULT 0 CHECK (credits_used >= 0),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, cycle_start),
    CHECK (cycle_end > cycle_start)
);

COMMENT ON TABLE solprint.gateway_billing_cycles IS
    'Per-provider billing cycle ledger maintained by GATEWAY (local accounting, not provider-reported usage).';

-- Per-minute usage, additive. `category` is a bounded label (method or
-- endpoint group), never a free-form value such as an address.
CREATE TABLE solprint.gateway_usage_buckets (
    provider     TEXT        NOT NULL CHECK (provider ~ '^[a-z_]{1,32}$'),
    bucket_start TIMESTAMPTZ NOT NULL,
    category     TEXT        NOT NULL CHECK (category ~ '^[A-Za-z0-9_.:-]{1,64}$'),
    requests     BIGINT      NOT NULL DEFAULT 0 CHECK (requests >= 0),
    credits      BIGINT      NOT NULL DEFAULT 0 CHECK (credits >= 0),
    bytes        BIGINT      NOT NULL DEFAULT 0 CHECK (bytes >= 0),
    PRIMARY KEY (provider, bucket_start, category)
);

CREATE INDEX gateway_usage_buckets_start_idx ON solprint.gateway_usage_buckets (bucket_start);

COMMENT ON TABLE solprint.gateway_usage_buckets IS
    'Per-minute request/credit/byte counters per provider and bounded category. Retention-pruned.';

-- Per-minute operation metrics. Latency is stored as a fixed log-scale
-- histogram (microsecond bucket upper bounds defined in solprint-gateway), so
-- percentiles can be recomputed over any window without raw samples.
CREATE TABLE solprint.gateway_metric_buckets (
    provider      TEXT        NOT NULL CHECK (provider ~ '^[a-z_]{1,32}$'),
    operation     TEXT        NOT NULL CHECK (operation ~ '^[A-Za-z0-9_.:-]{1,64}$'),
    bucket_start  TIMESTAMPTZ NOT NULL,
    success_count BIGINT      NOT NULL DEFAULT 0 CHECK (success_count >= 0),
    error_count   BIGINT      NOT NULL DEFAULT 0 CHECK (error_count >= 0),
    cache_hits    BIGINT      NOT NULL DEFAULT 0 CHECK (cache_hits >= 0),
    latency_hist  BIGINT[]    NOT NULL,
    PRIMARY KEY (provider, operation, bucket_start)
);

CREATE INDEX gateway_metric_buckets_start_idx ON solprint.gateway_metric_buckets (bucket_start);

COMMENT ON TABLE solprint.gateway_metric_buckets IS
    'Per-minute latency histograms and outcome counts per provider operation. Retention-pruned.';

-- Last observed position of each durable stream subscription, used to bound
-- gap recovery after a reconnect or restart.
CREATE TABLE solprint.gateway_stream_checkpoints (
    stream_key     TEXT        PRIMARY KEY CHECK (stream_key ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    kind           TEXT        NOT NULL CHECK (kind ~ '^[a-z_]{1,32}$'),
    commitment     TEXT        NOT NULL CHECK (commitment IN ('processed', 'confirmed', 'finalized')),
    last_slot      BIGINT      CHECK (last_slot >= 0),
    last_signature TEXT        CHECK (length(last_signature) BETWEEN 64 AND 128),
    continuity     TEXT        NOT NULL CHECK (continuity IN ('CONTINUOUS', 'RECOVERED', 'GAP', 'UNKNOWN')),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE solprint.gateway_stream_checkpoints IS
    'Last processed slot/signature per stream subscription, for bounded recovery after reconnects.';
