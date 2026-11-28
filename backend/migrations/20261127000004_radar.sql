-- Phase 03: RADAR, autonomous wallet discovery.
--
-- RADAR discovers markets, samples their transactions through the DATA ENGINE,
-- extracts evidence-backed traders and keeps a registry of research
-- candidates. It stores references to DATA ENGINE records (signatures, swap
-- ids, backfill ids), never copies of them. Nothing here can authorise a trade.
--
-- Time semantics: `*_discovered_at`, `observed_at` and `computed_at` are when
-- SOLPRINT learned something; `block_time` is when it happened on chain. A
-- wallet found today through last month's transactions is discovered today.
--
-- Forward-only. Never edit after it has been pushed.

-- DATA ENGINE targets may now be created by RADAR monitoring (still never
-- implicitly: only for wallets RADAR assigns to live monitoring).
ALTER TABLE solprint.de_targets DROP CONSTRAINT de_targets_discovery_source_check;
ALTER TABLE solprint.de_targets ADD CONSTRAINT de_targets_discovery_source_check
    CHECK (discovery_source IN ('MANUAL', 'LOCAL_DIAGNOSTIC', 'RADAR'));

-- Phase 02 gap: backfills paused by shutdown ('PAUSED') are claimable but were
-- missing from the runnable partial index.
DROP INDEX solprint.de_backfills_runnable_idx;
CREATE INDEX de_backfills_runnable_idx ON solprint.de_backfills (created_at)
    WHERE status IN ('PENDING', 'RUNNING', 'PAUSED', 'PAUSED_BY_BUDGET', 'PAUSED_BY_PROVIDER', 'PAUSED_BY_QUEUE');

-- ---- discovery cycles and source runs ------------------------------------------------------

CREATE TABLE solprint.radar_cycles (
    cycle_id           UUID                PRIMARY KEY,
    network            solprint.network_id NOT NULL,
    trigger            TEXT                NOT NULL CHECK (trigger IN ('SCHEDULED', 'OPERATOR', 'FIXTURE')),
    status             TEXT                NOT NULL CHECK (status IN ('RUNNING', 'COMPLETED', 'PARTIAL', 'DEFERRED', 'FAILED')),
    -- Seed of the exploration draw: the same seed and universe give the same picks.
    seed               BIGINT              NOT NULL,
    budget_state       TEXT                NOT NULL CHECK (budget_state ~ '^[A-Z_]{1,40}$'),
    deferred_reason    TEXT                CHECK (deferred_reason ~ '^[A-Z0-9_]{1,64}$'),
    observations       INTEGER             NOT NULL DEFAULT 0 CHECK (observations >= 0),
    markets_new        INTEGER             NOT NULL DEFAULT 0 CHECK (markets_new >= 0),
    markets_selected   INTEGER             NOT NULL DEFAULT 0 CHECK (markets_selected >= 0),
    samples_scheduled  INTEGER             NOT NULL DEFAULT 0 CHECK (samples_scheduled >= 0),
    credits_estimated  INTEGER             NOT NULL DEFAULT 0 CHECK (credits_estimated >= 0),
    started_at         TIMESTAMPTZ         NOT NULL DEFAULT now(),
    finished_at        TIMESTAMPTZ
);
CREATE INDEX radar_cycles_started_idx ON solprint.radar_cycles (started_at DESC);

CREATE TABLE solprint.radar_source_runs (
    run_id             UUID                PRIMARY KEY,
    cycle_id           UUID                NOT NULL REFERENCES solprint.radar_cycles (cycle_id) ON DELETE CASCADE,
    source             TEXT                NOT NULL CHECK (source ~ '^[A-Z0-9_]{1,48}$'),
    status             TEXT                NOT NULL CHECK (status IN ('OK', 'EMPTY', 'DISABLED', 'BUDGET_DEFERRED', 'PROVIDER_COOLDOWN', 'FAILED')),
    error_code         TEXT                CHECK (error_code ~ '^[A-Z0-9_]{1,64}$'),
    items              INTEGER             NOT NULL DEFAULT 0 CHECK (items >= 0),
    markets_new        INTEGER             NOT NULL DEFAULT 0 CHECK (markets_new >= 0),
    provider_requests  INTEGER             NOT NULL DEFAULT 0 CHECK (provider_requests >= 0),
    credits_estimated  INTEGER             NOT NULL DEFAULT 0 CHECK (credits_estimated >= 0),
    started_at         TIMESTAMPTZ         NOT NULL,
    finished_at        TIMESTAMPTZ         NOT NULL
);
CREATE INDEX radar_source_runs_source_idx ON solprint.radar_source_runs (source, started_at DESC);

-- ---- market universe ------------------------------------------------------------------------

-- One row per token mint (the only token identity; symbols and names are
-- untrusted display text). A market is a discovery target, not a recommendation.
CREATE TABLE solprint.radar_markets (
    network                 solprint.network_id   NOT NULL,
    mint                    solprint.sol_address  NOT NULL,
    token_program           solprint.sol_address,
    display_symbol          TEXT                  CHECK (char_length(display_symbol) <= 32),
    display_name            TEXT                  CHECK (char_length(display_name) <= 64),
    classification          TEXT                  NOT NULL CHECK (classification IN
                                ('MEME_CANDIDATE', 'GENERAL_SPECULATIVE_TOKEN', 'ESTABLISHED_ASSET', 'STABLECOIN',
                                 'WRAPPED_NATIVE_ASSET', 'LIQUID_STAKING_ASSET', 'LIQUIDITY_POSITION_TOKEN',
                                 'KNOWN_INFRASTRUCTURE_ASSET', 'UNKNOWN')),
    classification_reasons  TEXT[]                NOT NULL DEFAULT '{}',
    classification_ruleset  TEXT                  NOT NULL CHECK (classification_ruleset ~ '^[a-z0-9.-]{1,32}$'),
    bucket                  TEXT                  NOT NULL CHECK (bucket IN ('ESTABLISHED', 'EMERGING', 'LOCAL_ACTIVITY', 'HISTORICAL', 'EXPLORATION')),
    sources                 TEXT[]                NOT NULL DEFAULT '{}',
    -- True while every source that named this mint is promotional (paid boosts).
    promotional_only        BOOLEAN               NOT NULL DEFAULT FALSE,
    selection_state         TEXT                  NOT NULL DEFAULT 'CANDIDATE' CHECK (selection_state IN
                                ('CANDIDATE', 'SELECTED', 'DEFERRED', 'EXCLUDED', 'INACTIVE')),
    selection_reasons       TEXT[]                NOT NULL DEFAULT '{}',
    activity_state          TEXT                  NOT NULL DEFAULT 'UNKNOWN' CHECK (activity_state IN ('ACTIVE', 'QUIET', 'UNKNOWN')),
    -- 0..1, 6 dp. A scheduling weight, not a market quality rating.
    market_priority         NUMERIC(8, 6)         CHECK (market_priority BETWEEN 0 AND 1),
    coverage_state          TEXT                  NOT NULL DEFAULT 'NOT_SAMPLED' CHECK (coverage_state IN
                                ('NOT_SAMPLED', 'SAMPLED', 'NO_SUPPORTED_POOL', 'SAMPLE_FAILED')),
    enabled                 BOOLEAN               NOT NULL DEFAULT TRUE,
    samples_completed       INTEGER               NOT NULL DEFAULT 0 CHECK (samples_completed >= 0),
    wallets_found           INTEGER               NOT NULL DEFAULT 0 CHECK (wallets_found >= 0),
    first_discovered_at     TIMESTAMPTZ           NOT NULL DEFAULT now(),
    last_market_observation_at TIMESTAMPTZ,
    last_sampled_at         TIMESTAMPTZ,
    updated_at              TIMESTAMPTZ           NOT NULL DEFAULT now(),
    PRIMARY KEY (network, mint)
);
CREATE INDEX radar_markets_selection_idx ON solprint.radar_markets (selection_state, market_priority DESC NULLS LAST);
CREATE INDEX radar_markets_discovered_idx ON solprint.radar_markets (first_discovered_at DESC);

-- Liquidity pools of universe tokens. `program_id` is the on-chain owner
-- observed through getMultipleAccounts; a pool is sampled only when that owner
-- is a program the DATA ENGINE decodes.
CREATE TABLE solprint.radar_pools (
    network                 solprint.network_id   NOT NULL,
    pool                    solprint.sol_address  NOT NULL,
    mint                    solprint.sol_address  NOT NULL,
    quote_mint              solprint.sol_address,
    dex_id                  TEXT                  CHECK (dex_id ~ '^[a-z0-9_.-]{1,48}$'),
    labels                  TEXT[]                NOT NULL DEFAULT '{}',
    validation              TEXT                  NOT NULL DEFAULT 'UNVERIFIED' CHECK (validation IN
                                ('UNVERIFIED', 'SUPPORTED', 'UNSUPPORTED_PROGRAM', 'ACCOUNT_NOT_FOUND')),
    program_id              solprint.sol_address,
    protocol                TEXT                  CHECK (protocol ~ '^[A-Z0-9_]{1,32}$'),
    validated_at            TIMESTAMPTZ,
    -- Latest reported values (USD figures are provider-reported decimals).
    liquidity_usd           NUMERIC(38, 6),
    volume_24h_usd          NUMERIC(38, 6),
    txns_1h                 INTEGER               CHECK (txns_1h >= 0),
    txns_24h                INTEGER               CHECK (txns_24h >= 0),
    pair_created_at         TIMESTAMPTZ,
    last_observed_at        TIMESTAMPTZ           NOT NULL,
    last_sampled_at         TIMESTAMPTZ,
    -- Newest signature already sampled: the next sample stops there (no overlap).
    sample_until_signature  solprint.sol_signature,
    samples_completed       INTEGER               NOT NULL DEFAULT 0 CHECK (samples_completed >= 0),
    PRIMARY KEY (network, pool),
    FOREIGN KEY (network, mint) REFERENCES solprint.radar_markets (network, mint) ON DELETE CASCADE
);
CREATE INDEX radar_pools_mint_idx ON solprint.radar_pools (network, mint);

-- Append-only market observations: the universe as RADAR saw it at each cycle.
CREATE TABLE solprint.radar_market_observations (
    observation_id   BIGSERIAL             PRIMARY KEY,
    network          solprint.network_id   NOT NULL,
    cycle_id         UUID                  REFERENCES solprint.radar_cycles (cycle_id) ON DELETE SET NULL,
    mint             solprint.sol_address  NOT NULL,
    pool             solprint.sol_address,
    source           TEXT                  NOT NULL CHECK (source ~ '^[A-Z0-9_]{1,48}$'),
    promotional      BOOLEAN               NOT NULL DEFAULT FALSE,
    source_rank      INTEGER               CHECK (source_rank >= 0),
    liquidity_usd    NUMERIC(38, 6),
    volume_24h_usd   NUMERIC(38, 6),
    txns_1h          INTEGER               CHECK (txns_1h >= 0),
    txns_24h         INTEGER               CHECK (txns_24h >= 0),
    pair_created_at  TIMESTAMPTZ,
    organic_score    NUMERIC(10, 4),
    decision         TEXT                  CHECK (decision IN ('SELECTED', 'DEFERRED', 'EXCLUDED', 'OBSERVED')),
    decision_reasons TEXT[]                NOT NULL DEFAULT '{}',
    observed_at      TIMESTAMPTZ           NOT NULL
);
CREATE INDEX radar_market_observations_mint_idx ON solprint.radar_market_observations (network, mint, observed_at DESC);
CREATE INDEX radar_market_observations_time_idx ON solprint.radar_market_observations (observed_at);

-- ---- jobs -------------------------------------------------------------------------------------

CREATE TABLE solprint.radar_jobs (
    job_id             UUID                PRIMARY KEY,
    network            solprint.network_id NOT NULL,
    work_class         TEXT                NOT NULL CHECK (work_class IN
                           ('TRANSACTION_SAMPLING', 'INITIAL_HISTORY', 'EXPANDED_HISTORY', 'EXTENDED_HISTORY',
                            'ACTIVE_MONITORING', 'DORMANT_RECHECK')),
    subject_kind       TEXT                NOT NULL CHECK (subject_kind IN ('POOL', 'WALLET')),
    subject            solprint.sol_address NOT NULL,
    origin             TEXT                NOT NULL CHECK (origin IN ('AUTONOMOUS', 'OPERATOR')),
    bucket             TEXT                CHECK (bucket IN ('ESTABLISHED', 'EMERGING', 'LOCAL_ACTIVITY', 'HISTORICAL', 'EXPLORATION')),
    cycle_id           UUID                REFERENCES solprint.radar_cycles (cycle_id) ON DELETE SET NULL,
    -- Higher runs first (0..1000).
    priority           INTEGER             NOT NULL CHECK (priority BETWEEN 0 AND 1000),
    state              TEXT                NOT NULL CHECK (state IN
                           ('PENDING', 'RUNNING', 'WAITING_DATA', 'DEFERRED', 'COMPLETED', 'FAILED', 'CANCELLED')),
    defer_reason       TEXT                CHECK (defer_reason ~ '^[A-Z0-9_]{1,64}$'),
    result_code        TEXT                CHECK (result_code ~ '^[A-Z0-9_]{1,64}$'),
    signature_limit    INTEGER             NOT NULL CHECK (signature_limit BETWEEN 1 AND 10000),
    lookback_days      INTEGER             CHECK (lookback_days BETWEEN 1 AND 365),
    start_before_signature solprint.sol_signature,
    until_signature    solprint.sol_signature,
    min_slot           BIGINT              CHECK (min_slot >= 0),
    commitment         TEXT                NOT NULL CHECK (commitment IN ('confirmed', 'finalized')),
    backfill_id        UUID                REFERENCES solprint.de_backfills (backfill_id) ON DELETE SET NULL,
    estimated_credits  INTEGER             NOT NULL CHECK (estimated_credits >= 0),
    -- Estimated credits spent: signature pages plus one per queued transaction.
    credits_spent      INTEGER             NOT NULL DEFAULT 0 CHECK (credits_spent >= 0),
    signatures_found   INTEGER             NOT NULL DEFAULT 0 CHECK (signatures_found >= 0),
    signatures_reused  INTEGER             NOT NULL DEFAULT 0 CHECK (signatures_reused >= 0),
    attempts           INTEGER             NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    cancel_requested   BOOLEAN             NOT NULL DEFAULT FALSE,
    next_run_at        TIMESTAMPTZ         NOT NULL DEFAULT now(),
    lease_owner        UUID,
    lease_expires_at   TIMESTAMPTZ,
    created_at         TIMESTAMPTZ         NOT NULL DEFAULT now(),
    started_at         TIMESTAMPTZ,
    completed_at       TIMESTAMPTZ,
    updated_at         TIMESTAMPTZ         NOT NULL DEFAULT now(),
    CHECK ((state = 'RUNNING') = (lease_owner IS NOT NULL))
);
-- At most one open job per (class, subject): duplicate investigations are impossible.
CREATE UNIQUE INDEX radar_jobs_open_subject_idx ON solprint.radar_jobs (network, work_class, subject)
    WHERE state IN ('PENDING', 'RUNNING', 'WAITING_DATA', 'DEFERRED');
CREATE INDEX radar_jobs_runnable_idx ON solprint.radar_jobs (next_run_at, priority DESC)
    WHERE state IN ('PENDING', 'DEFERRED', 'WAITING_DATA');
CREATE INDEX radar_jobs_subject_idx ON solprint.radar_jobs (network, subject, created_at DESC);
CREATE INDEX radar_jobs_backfill_idx ON solprint.radar_jobs (backfill_id) WHERE backfill_id IS NOT NULL;
CREATE INDEX radar_jobs_completed_idx ON solprint.radar_jobs (completed_at) WHERE completed_at IS NOT NULL;

-- ---- wallet registry --------------------------------------------------------------------------

CREATE TABLE solprint.radar_wallets (
    wallet_id                UUID                 NOT NULL UNIQUE,
    network                  solprint.network_id  NOT NULL,
    address                  solprint.sol_address NOT NULL,
    lifecycle                TEXT                 NOT NULL CHECK (lifecycle IN
                                 ('DISCOVERED', 'SCREENING', 'HISTORY_QUEUED', 'INVESTIGATING', 'INSUFFICIENT_EVIDENCE',
                                  'PRELIMINARY_QUALIFIED', 'PRIORITIZED', 'MONITORED', 'DORMANT', 'REJECTED', 'ARCHIVED')),
    eligibility              TEXT                 NOT NULL CHECK (eligibility IN
                                 ('ELIGIBLE_CANDIDATE', 'PROVISIONAL_CANDIDATE', 'NEEDS_MORE_EVIDENCE', 'IRRELEVANT_ACTIVITY',
                                  'KNOWN_INFRASTRUCTURE', 'AMBIGUOUS_ATTRIBUTION', 'UNSUPPORTED_ACCOUNT_ROLE', 'EXCLUDED_BY_POLICY')),
    eligibility_reasons      TEXT[]               NOT NULL DEFAULT '{}',
    qualification            TEXT                 CHECK (qualification IN
                                 ('INSUFFICIENT_ACTIVITY', 'INSUFFICIENT_HISTORY', 'INSUFFICIENT_FINANCIAL_EVIDENCE',
                                  'PRELIMINARY_POSITIVE_RETURN', 'PRELIMINARY_NEGATIVE_RETURN', 'RECENT_ACTIVITY_QUALIFIED',
                                  'READY_FOR_DEEPER_ANALYSIS')),
    qualification_reasons    TEXT[]               NOT NULL DEFAULT '{}',
    -- 0..100, 4 dp. An investigation-priority weight, not a trader-quality rating.
    priority_score           NUMERIC(7, 4)        CHECK (priority_score BETWEEN 0 AND 100),
    priority_policy_version  TEXT                 CHECK (priority_policy_version ~ '^[a-z0-9.-]{1,32}$'),
    history_stage            TEXT                 NOT NULL DEFAULT 'NONE' CHECK (history_stage IN ('NONE', 'A', 'B', 'C')),
    history_coverage_state   TEXT                 NOT NULL DEFAULT 'NOT_INVESTIGATED' CHECK (history_coverage_state IN
                                 ('NOT_INVESTIGATED', 'INVESTIGATION_PENDING', 'PARTIAL_HISTORY', 'BOUNDED_SAMPLE_COMPLETE',
                                  'REQUESTED_SCOPE_COMPLETE', 'PROVIDER_HISTORY_UNAVAILABLE', 'DECODER_COVERAGE_LIMITED',
                                  'UNRESOLVED_GAPS', 'UNKNOWN')),
    history_coverage_start   TIMESTAMPTZ,
    history_coverage_end     TIMESTAMPTZ,
    monitoring_mode          TEXT                 NOT NULL DEFAULT 'INACTIVE' CHECK (monitoring_mode IN
                                 ('LIVE_SUBSCRIPTION', 'PERIODIC_HISTORY_REFRESH', 'PASSIVE_LOCAL_OBSERVATION', 'DEFERRED', 'INACTIVE')),
    first_discovered_at      TIMESTAMPTZ          NOT NULL,
    first_discovery_source   TEXT                 NOT NULL CHECK (first_discovery_source ~ '^[A-Z0-9_]{1,48}$'),
    last_discovered_at       TIMESTAMPTZ          NOT NULL,
    last_verified_trade_at   TIMESTAMPTZ,
    last_observed_at         TIMESTAMPTZ,
    last_investigated_at     TIMESTAMPTZ,
    last_evaluated_at        TIMESTAMPTZ,
    next_investigation_at    TIMESTAMPTZ,
    discovery_evidence_count INTEGER              NOT NULL DEFAULT 0 CHECK (discovery_evidence_count >= 0),
    valid_evidence_count     INTEGER              NOT NULL DEFAULT 0 CHECK (valid_evidence_count >= 0),
    verified_swap_count      INTEGER              NOT NULL DEFAULT 0 CHECK (verified_swap_count >= 0),
    distinct_token_count     INTEGER              NOT NULL DEFAULT 0 CHECK (distinct_token_count >= 0),
    operator_excluded        BOOLEAN              NOT NULL DEFAULT FALSE,
    -- Set by evidence changes; the evaluator recomputes only dirty wallets.
    needs_evaluation         BOOLEAN              NOT NULL DEFAULT TRUE,
    revision                 INTEGER              NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at               TIMESTAMPTZ          NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ          NOT NULL DEFAULT now(),
    PRIMARY KEY (network, address)
);
CREATE INDEX radar_wallets_lifecycle_idx ON solprint.radar_wallets (lifecycle, priority_score DESC NULLS LAST);
CREATE INDEX radar_wallets_priority_idx ON solprint.radar_wallets (priority_score DESC NULLS LAST, address);
CREATE INDEX radar_wallets_dirty_idx ON solprint.radar_wallets (updated_at) WHERE needs_evaluation;
CREATE INDEX radar_wallets_discovered_idx ON solprint.radar_wallets (first_discovered_at DESC);
CREATE INDEX radar_wallets_next_idx ON solprint.radar_wallets (next_investigation_at) WHERE next_investigation_at IS NOT NULL;

-- Why each wallet is known: one row per (wallet, swap). Invalidated evidence is
-- kept with its reason, never deleted.
CREATE TABLE solprint.radar_wallet_evidence (
    evidence_id         BIGSERIAL               PRIMARY KEY,
    network             solprint.network_id     NOT NULL,
    address             solprint.sol_address    NOT NULL,
    swap_id             TEXT                    NOT NULL CHECK (length(swap_id) <= 160),
    signature           solprint.sol_signature  NOT NULL,
    slot                BIGINT                  NOT NULL CHECK (slot >= 0),
    block_time          TIMESTAMPTZ,
    token_mint          solprint.sol_address,
    quote_mint          solprint.sol_address,
    pool                solprint.sol_address,
    protocol            TEXT                    NOT NULL CHECK (protocol ~ '^[A-Z0-9_]{1,32}$'),
    economic_role       TEXT                    NOT NULL CHECK (economic_role IN ('BUYER', 'SELLER', 'SWAPPER')),
    attribution         TEXT                    NOT NULL CHECK (attribution ~ '^[A-Z_]{1,32}$'),
    finality            TEXT                    NOT NULL CHECK (finality ~ '^[A-Z_]{1,32}$'),
    discovery_source    TEXT                    NOT NULL CHECK (discovery_source ~ '^[A-Z0-9_]{1,48}$'),
    job_id              UUID,
    backfill_id         UUID,
    swap_revision       INTEGER                 NOT NULL CHECK (swap_revision >= 1),
    valid               BOOLEAN                 NOT NULL DEFAULT TRUE,
    invalidation_reason TEXT                    CHECK (invalidation_reason ~ '^[A-Z0-9_]{1,64}$'),
    invalidated_at      TIMESTAMPTZ,
    -- When the DATA ENGINE published the swap, and when RADAR recorded it.
    evidence_available_at TIMESTAMPTZ           NOT NULL,
    discovered_at       TIMESTAMPTZ             NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ             NOT NULL DEFAULT now(),
    UNIQUE (network, address, swap_id),
    FOREIGN KEY (network, address) REFERENCES solprint.radar_wallets (network, address) ON DELETE CASCADE,
    CHECK (valid = (invalidated_at IS NULL))
);
CREATE INDEX radar_wallet_evidence_swap_idx ON solprint.radar_wallet_evidence (swap_id);
CREATE INDEX radar_wallet_evidence_signature_idx ON solprint.radar_wallet_evidence (network, signature);
CREATE INDEX radar_wallet_evidence_wallet_idx ON solprint.radar_wallet_evidence (network, address, block_time DESC);

CREATE TABLE solprint.radar_wallet_transitions (
    transition_id   BIGSERIAL             PRIMARY KEY,
    network         solprint.network_id   NOT NULL,
    address         solprint.sol_address  NOT NULL,
    from_state      TEXT,
    to_state        TEXT                  NOT NULL,
    trigger         TEXT                  NOT NULL CHECK (trigger ~ '^[A-Z0-9_]{1,64}$'),
    reasons         TEXT[]                NOT NULL DEFAULT '{}',
    metrics         JSONB                 NOT NULL DEFAULT '{}' CHECK (octet_length(metrics::text) <= 4096),
    policy_version  TEXT                  NOT NULL,
    transitioned_at TIMESTAMPTZ           NOT NULL DEFAULT now(),
    FOREIGN KEY (network, address) REFERENCES solprint.radar_wallets (network, address) ON DELETE CASCADE
);
CREATE INDEX radar_wallet_transitions_wallet_idx ON solprint.radar_wallet_transitions (network, address, transition_id DESC);

-- Versioned preliminary metrics, priority components and qualification as
-- computed at `computed_at`. Written only when the inputs changed.
CREATE TABLE solprint.radar_wallet_snapshots (
    snapshot_id     BIGSERIAL             PRIMARY KEY,
    network         solprint.network_id   NOT NULL,
    address         solprint.sol_address  NOT NULL,
    policy_version  TEXT                  NOT NULL,
    input_hash      TEXT                  NOT NULL CHECK (input_hash ~ '^[0-9a-f]{64}$'),
    priority_score  NUMERIC(7, 4)         NOT NULL CHECK (priority_score BETWEEN 0 AND 100),
    qualification   TEXT                  NOT NULL,
    metrics         JSONB                 NOT NULL CHECK (octet_length(metrics::text) <= 65536),
    computed_at     TIMESTAMPTZ           NOT NULL DEFAULT now(),
    FOREIGN KEY (network, address) REFERENCES solprint.radar_wallets (network, address) ON DELETE CASCADE
);
CREATE INDEX radar_wallet_snapshots_wallet_idx ON solprint.radar_wallet_snapshots (network, address, computed_at DESC);

CREATE TABLE solprint.radar_monitoring (
    assignment_id   BIGSERIAL             PRIMARY KEY,
    network         solprint.network_id   NOT NULL,
    address         solprint.sol_address  NOT NULL,
    mode            TEXT                  NOT NULL CHECK (mode IN ('LIVE_SUBSCRIPTION', 'PERIODIC_HISTORY_REFRESH', 'PASSIVE_LOCAL_OBSERVATION')),
    target_id       UUID                  REFERENCES solprint.de_targets (target_id) ON DELETE SET NULL,
    priority_at_start NUMERIC(7, 4),
    started_at      TIMESTAMPTZ           NOT NULL DEFAULT now(),
    ended_at        TIMESTAMPTZ,
    end_reason      TEXT                  CHECK (end_reason ~ '^[A-Z0-9_]{1,64}$'),
    FOREIGN KEY (network, address) REFERENCES solprint.radar_wallets (network, address) ON DELETE CASCADE
);
CREATE UNIQUE INDEX radar_monitoring_active_idx ON solprint.radar_monitoring (network, address) WHERE ended_at IS NULL;
CREATE INDEX radar_monitoring_ended_idx ON solprint.radar_monitoring (network, address, ended_at DESC);

-- ---- control, operator actions and RADAR domain events -------------------------------------

CREATE TABLE solprint.radar_control (
    network          solprint.network_id PRIMARY KEY,
    paused           BOOLEAN             NOT NULL DEFAULT FALSE,
    pause_reason     TEXT                CHECK (pause_reason ~ '^[A-Z0-9_]{1,64}$'),
    cycle_requested  BOOLEAN             NOT NULL DEFAULT FALSE,
    last_cycle_at    TIMESTAMPTZ,
    updated_at       TIMESTAMPTZ         NOT NULL DEFAULT now()
);

CREATE TABLE solprint.radar_operator_actions (
    action_id   BIGSERIAL            PRIMARY KEY,
    network     solprint.network_id  NOT NULL,
    action      TEXT                 NOT NULL CHECK (action ~ '^[A-Z_]{1,48}$'),
    subject     TEXT                 CHECK (length(subject) <= 128),
    outcome     TEXT                 NOT NULL CHECK (outcome ~ '^[A-Z_]{1,48}$'),
    acted_at    TIMESTAMPTZ          NOT NULL DEFAULT now()
);

-- Same contract as de_outbox (atomic with the change, stable id, schema
-- version, aggregate revision, at-least-once delivery), kept separate so
-- RADAR never consumes its own events and the DATA ENGINE never sees them.
CREATE TABLE solprint.radar_outbox (
    event_id            UUID                PRIMARY KEY,
    seq                 BIGSERIAL           NOT NULL UNIQUE,
    event_type          TEXT                NOT NULL CHECK (event_type ~ '^[A-Za-z]{1,64}$'),
    schema_version      SMALLINT            NOT NULL CHECK (schema_version >= 1),
    aggregate_type      TEXT                NOT NULL CHECK (aggregate_type IN ('WALLET', 'MARKET')),
    aggregate_id        TEXT                NOT NULL CHECK (length(aggregate_id) <= 160),
    aggregate_revision  INTEGER             NOT NULL CHECK (aggregate_revision >= 1),
    network             solprint.network_id NOT NULL,
    correlation_id      UUID                NOT NULL,
    causation_id        UUID,
    source              TEXT                NOT NULL CHECK (source ~ '^[A-Z0-9_]{1,48}$'),
    observed_at         TIMESTAMPTZ         NOT NULL,
    persisted_at        TIMESTAMPTZ         NOT NULL DEFAULT now(),
    payload             JSONB               NOT NULL CHECK (octet_length(payload::text) <= 16384),
    UNIQUE (aggregate_type, aggregate_id, aggregate_revision, event_type)
);
CREATE INDEX radar_outbox_persisted_idx ON solprint.radar_outbox (persisted_at);

CREATE TABLE solprint.radar_outbox_deliveries (
    consumer      TEXT        NOT NULL CHECK (consumer ~ '^[a-z0-9_.]{1,64}$'),
    event_id      UUID        NOT NULL REFERENCES solprint.radar_outbox (event_id) ON DELETE CASCADE,
    delivered_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (consumer, event_id)
);

COMMENT ON TABLE solprint.radar_wallets IS
    'RADAR candidate registry: research candidates with provenance. A candidate is never a trading signal.';
COMMENT ON TABLE solprint.radar_wallet_evidence IS
    'Evidence-backed discovery observations (DATA ENGINE swap references). Invalidated rows are kept.';
