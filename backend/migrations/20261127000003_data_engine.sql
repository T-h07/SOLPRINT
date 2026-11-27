-- DATA ENGINE persistence (Phase 02).
--
-- Durable ingestion queue, retrieved transaction evidence, normalized
-- activities and swaps, finality, historical backfills, data-quality issues
-- and the transactional domain-event outbox.
--
-- Numeric conventions (docs/architecture/DATA_CONVENTIONS.md):
--   * On-chain unsigned 64-bit quantities (lamports, raw token amounts, fees)
--     are NUMERIC(20,0) with a range CHECK. PostgreSQL BIGINT is signed and
--     cannot hold the upper half of the u64 range.
--   * Signed balance deltas are NUMERIC(21,0) (|delta| <= u64::MAX).
--   * Ratios and prices are NUMERIC with an explicit scale of 18.
--   * Slots fit in BIGINT (< 2^63 for any realistic chain lifetime) and are
--     checked to be non-negative.
--
-- Append-only: never edit this file after it has been applied anywhere.

-- ---- shared domains --------------------------------------------------------

CREATE DOMAIN solprint.u64_amount AS NUMERIC(20, 0)
    CHECK (VALUE >= 0 AND VALUE <= 18446744073709551615);

CREATE DOMAIN solprint.i65_delta AS NUMERIC(21, 0)
    CHECK (VALUE >= -18446744073709551615 AND VALUE <= 18446744073709551615);

CREATE DOMAIN solprint.sol_address AS TEXT
    CHECK (VALUE ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$');

CREATE DOMAIN solprint.sol_signature AS TEXT
    CHECK (VALUE ~ '^[1-9A-HJ-NP-Za-km-z]{64,88}$');

CREATE DOMAIN solprint.network_id AS TEXT
    CHECK (VALUE IN ('solana-mainnet'));

-- ---- ingestion targets -----------------------------------------------------

CREATE TABLE solprint.de_targets (
    target_id                UUID                  PRIMARY KEY,
    network                  solprint.network_id   NOT NULL,
    target_type              TEXT                  NOT NULL CHECK (target_type IN
                                 ('WALLET_ADDRESS', 'TOKEN_MINT', 'POOL_ADDRESS', 'PROGRAM_ADDRESS', 'TRANSACTION_SIGNATURE')),
    address                  TEXT                  NOT NULL CHECK (address ~ '^[1-9A-HJ-NP-Za-km-z]{32,88}$'),
    label                    TEXT                  CHECK (length(label) BETWEEN 1 AND 64),
    enabled                  BOOLEAN               NOT NULL DEFAULT TRUE,
    priority                 TEXT                  NOT NULL DEFAULT 'P2' CHECK (priority IN ('P2', 'P3', 'P4', 'P5')),
    commitment               TEXT                  NOT NULL DEFAULT 'confirmed' CHECK (commitment IN ('confirmed', 'finalized')),
    discovery_source         TEXT                  NOT NULL CHECK (discovery_source IN ('MANUAL', 'LOCAL_DIAGNOSTIC')),
    created_at               TIMESTAMPTZ           NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ           NOT NULL DEFAULT now(),
    last_observation_at      TIMESTAMPTZ,
    history_oldest_slot      BIGINT                CHECK (history_oldest_slot >= 0),
    history_newest_slot      BIGINT                CHECK (history_newest_slot >= 0),
    history_exhausted        BOOLEAN               NOT NULL DEFAULT FALSE,
    coverage_state           TEXT                  NOT NULL DEFAULT 'NONE' CHECK (coverage_state IN
                                 ('NONE', 'LIVE_ONLY', 'PARTIAL_HISTORY', 'ADDRESS_HISTORY_EXHAUSTED', 'GAPS_UNRESOLVED')),
    UNIQUE (network, target_type, address)
);

COMMENT ON TABLE solprint.de_targets IS
    'Explicitly configured ingestion targets. No target is ever created automatically.';

-- ---- observations ------------------------------------------------------------

-- Every sighting of a signature, by source. Several rows can exist for one
-- transaction (live + recovery + backfill + manual); the transaction is still
-- fetched and decoded once.
CREATE TABLE solprint.de_observations (
    observation_id    BIGSERIAL               PRIMARY KEY,
    network           solprint.network_id     NOT NULL,
    signature         solprint.sol_signature  NOT NULL,
    source            TEXT                    NOT NULL CHECK (source IN ('LIVE_STREAM', 'STREAM_RECOVERY', 'BACKFILL', 'MANUAL')),
    target_id         UUID                    REFERENCES solprint.de_targets (target_id) ON DELETE SET NULL,
    backfill_id       UUID,
    commitment        TEXT                    CHECK (commitment IN ('processed', 'confirmed', 'finalized')),
    slot              BIGINT                  CHECK (slot >= 0),
    failed_hint       BOOLEAN,
    stream_generation BIGINT                  CHECK (stream_generation >= 0),
    observed_at       TIMESTAMPTZ             NOT NULL,
    dedup_key         TEXT                    NOT NULL UNIQUE CHECK (length(dedup_key) <= 256)
);

CREATE INDEX de_observations_signature_idx ON solprint.de_observations (network, signature);
CREATE INDEX de_observations_target_idx ON solprint.de_observations (target_id, observed_at DESC);

COMMENT ON TABLE solprint.de_observations IS
    'Source observations of transaction signatures (provenance). Deduplicated per signature, source, target and commitment.';

-- ---- durable ingestion queue -----------------------------------------------------

CREATE TABLE solprint.de_jobs (
    job_id              BIGSERIAL               PRIMARY KEY,
    network             solprint.network_id     NOT NULL,
    signature           solprint.sol_signature  NOT NULL,
    state               TEXT                    NOT NULL CHECK (state IN
                            ('PENDING', 'FETCHING', 'FETCH_RETRY', 'COMPLETED', 'UNAVAILABLE', 'UNSUPPORTED', 'QUARANTINED', 'CANCELLED')),
    priority            SMALLINT                NOT NULL CHECK (priority BETWEEN 0 AND 5),
    required_commitment TEXT                    NOT NULL CHECK (required_commitment IN ('confirmed', 'finalized')),
    source              TEXT                    NOT NULL CHECK (source IN ('LIVE_STREAM', 'STREAM_RECOVERY', 'BACKFILL', 'MANUAL', 'FINALITY', 'REPROCESS')),
    attempts            INTEGER                 NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts        INTEGER                 NOT NULL CHECK (max_attempts BETWEEN 1 AND 20),
    next_attempt_at     TIMESTAMPTZ             NOT NULL DEFAULT now(),
    lease_owner         UUID,
    lease_expires_at    TIMESTAMPTZ,
    last_error_code     TEXT                    CHECK (last_error_code ~ '^[A-Z0-9_]{1,64}$'),
    result_revision     INTEGER                 CHECK (result_revision >= 1),
    created_at          TIMESTAMPTZ             NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ             NOT NULL DEFAULT now(),
    completed_at        TIMESTAMPTZ,
    UNIQUE (network, signature),
    CHECK ((state = 'FETCHING') = (lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL))
);

CREATE INDEX de_jobs_claimable_idx ON solprint.de_jobs (priority, next_attempt_at, job_id)
    WHERE state IN ('PENDING', 'FETCH_RETRY');
CREATE INDEX de_jobs_lease_idx ON solprint.de_jobs (lease_expires_at) WHERE state = 'FETCHING';
CREATE INDEX de_jobs_completed_idx ON solprint.de_jobs (completed_at) WHERE completed_at IS NOT NULL;

COMMENT ON TABLE solprint.de_jobs IS
    'Durable transaction retrieval queue: one row per signature, leased by bounded workers (FOR UPDATE SKIP LOCKED).';

-- ---- transactions and evidence ---------------------------------------------------

CREATE TABLE solprint.de_transactions (
    network                 solprint.network_id     NOT NULL,
    signature               solprint.sol_signature  NOT NULL,
    slot                    BIGINT                  NOT NULL CHECK (slot >= 0),
    block_time              TIMESTAMPTZ,
    transaction_index       INTEGER                 CHECK (transaction_index >= 0),
    version                 TEXT                    NOT NULL CHECK (version ~ '^(legacy|[0-9]{1,3})$'),
    execution_status        TEXT                    NOT NULL CHECK (execution_status IN ('SUCCESS', 'FAILED')),
    err_json                TEXT                    CHECK (length(err_json) <= 512),
    fee_lamports            solprint.u64_amount     NOT NULL,
    fee_payer               solprint.sol_address    NOT NULL,
    signers                 TEXT[]                  NOT NULL,
    compute_units           solprint.u64_amount,
    finality                TEXT                    NOT NULL CHECK (finality IN
                                ('OBSERVED_PROCESSED', 'OBSERVED_CONFIRMED', 'FINALIZED', 'FINALITY_UNKNOWN', 'POSSIBLY_ORPHANED', 'ORPHANED')),
    finality_checks         INTEGER                 NOT NULL DEFAULT 0 CHECK (finality_checks >= 0),
    next_finality_check_at  TIMESTAMPTZ,
    finalized_at            TIMESTAMPTZ,
    current_revision        INTEGER                 NOT NULL CHECK (current_revision >= 1),
    decode_status           TEXT                    NOT NULL CHECK (decode_status IN
                                ('VERIFIED', 'PARTIALLY_DECODED', 'AMBIGUOUS', 'UNSUPPORTED', 'INSUFFICIENT_EVIDENCE', 'INVALID')),
    decoder_version         TEXT                    NOT NULL CHECK (length(decoder_version) BETWEEN 1 AND 32),
    protocols               TEXT[]                  NOT NULL DEFAULT '{}',
    activity_kinds          TEXT[]                  NOT NULL DEFAULT '{}',
    swap_count              INTEGER                 NOT NULL DEFAULT 0 CHECK (swap_count >= 0),
    first_observed_at       TIMESTAMPTZ             NOT NULL,
    evidence_observed_at    TIMESTAMPTZ             NOT NULL,
    decoded_at              TIMESTAMPTZ             NOT NULL,
    persisted_at            TIMESTAMPTZ             NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ             NOT NULL DEFAULT now(),
    PRIMARY KEY (network, signature)
);

CREATE INDEX de_transactions_time_idx ON solprint.de_transactions (block_time DESC NULLS LAST, slot DESC);
CREATE INDEX de_transactions_slot_idx ON solprint.de_transactions (slot DESC);
CREATE INDEX de_transactions_finality_due_idx ON solprint.de_transactions (next_finality_check_at)
    WHERE next_finality_check_at IS NOT NULL;
CREATE INDEX de_transactions_protocols_idx ON solprint.de_transactions USING GIN (protocols);

COMMENT ON TABLE solprint.de_transactions IS
    'One row per retrieved transaction: identity, execution status, fee, finality and the current evidence revision.';

-- Every retrieval of a transaction is a revision. Raw JSON is kept for
-- reprocessing and audit until retention prunes it (the hash stays).
CREATE TABLE solprint.de_evidence (
    network            solprint.network_id     NOT NULL,
    signature          solprint.sol_signature  NOT NULL,
    revision           INTEGER                 NOT NULL CHECK (revision >= 1),
    commitment         TEXT                    NOT NULL CHECK (commitment IN ('confirmed', 'finalized')),
    provider           TEXT                    NOT NULL CHECK (provider IN ('HELIUS', 'SOLANA_RPC', 'FIXTURE')),
    slot               BIGINT                  NOT NULL CHECK (slot >= 0),
    fetched_at         TIMESTAMPTZ             NOT NULL,
    evidence_schema    SMALLINT                NOT NULL CHECK (evidence_schema >= 1),
    content_sha256     TEXT                    NOT NULL CHECK (content_sha256 ~ '^[0-9a-f]{64}$'),
    byte_len           INTEGER                 NOT NULL CHECK (byte_len >= 0),
    raw_json           TEXT,
    pruned_at          TIMESTAMPTZ,
    canonical          BOOLEAN                 NOT NULL DEFAULT TRUE,
    superseded_reason  TEXT                    CHECK (superseded_reason IN ('STRONGER_COMMITMENT', 'ORPHANED', 'REFETCHED')),
    PRIMARY KEY (network, signature, revision),
    FOREIGN KEY (network, signature) REFERENCES solprint.de_transactions (network, signature) ON DELETE CASCADE,
    CHECK ((raw_json IS NULL) = (pruned_at IS NOT NULL))
);

COMMENT ON TABLE solprint.de_evidence IS
    'Retrieved transaction evidence revisions: raw getTransaction JSON (bounded, prunable), its SHA-256 and provenance.';

-- Account-level native and token balance changes of the current revision.
CREATE TABLE solprint.de_balance_changes (
    network             solprint.network_id     NOT NULL,
    signature           solprint.sol_signature  NOT NULL,
    account_index       SMALLINT                NOT NULL CHECK (account_index >= 0),
    asset               TEXT                    NOT NULL CHECK (asset = 'SOL' OR asset ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'),
    account             solprint.sol_address    NOT NULL,
    owner               solprint.sol_address,
    token_program       solprint.sol_address,
    decimals            SMALLINT                CHECK (decimals BETWEEN 0 AND 255),
    pre_amount          solprint.u64_amount,
    post_amount         solprint.u64_amount,
    delta               solprint.i65_delta,
    pre_evidence        TEXT                    NOT NULL CHECK (pre_evidence IN ('REPORTED', 'INFERRED_CREATED', 'UNKNOWN')),
    post_evidence       TEXT                    NOT NULL CHECK (post_evidence IN ('REPORTED', 'INFERRED_CLOSED', 'UNKNOWN')),
    ownership_evidence  TEXT                    NOT NULL CHECK (ownership_evidence IN
                            ('NATIVE_ACCOUNT', 'TOKEN_BALANCE_META', 'INITIALIZE_ACCOUNT', 'ASSOCIATED_ACCOUNT_CREATE', 'UNKNOWN')),
    PRIMARY KEY (network, signature, account_index, asset),
    FOREIGN KEY (network, signature) REFERENCES solprint.de_transactions (network, signature) ON DELETE CASCADE
);

CREATE INDEX de_balance_changes_owner_idx ON solprint.de_balance_changes (owner, asset) WHERE owner IS NOT NULL;

-- ---- normalized activities --------------------------------------------------------

CREATE TABLE solprint.de_activities (
    activity_id         TEXT                    PRIMARY KEY CHECK (length(activity_id) <= 160),
    network             solprint.network_id     NOT NULL,
    signature           solprint.sol_signature  NOT NULL,
    activity_index      INTEGER                 NOT NULL CHECK (activity_index >= 0),
    kind                TEXT                    NOT NULL CHECK (kind IN
                            ('SWAP', 'TOKEN_TRANSFER', 'NATIVE_TRANSFER', 'TOKEN_MINT', 'TOKEN_BURN', 'LIQUIDITY_DEPOSIT',
                             'LIQUIDITY_WITHDRAWAL', 'POOL_CREATION', 'BONDING_CURVE_TRADE', 'LIQUIDITY_MIGRATION',
                             'ACCOUNT_CREATION', 'ACCOUNT_CLOSURE', 'WRAP_SOL', 'UNWRAP_SOL', 'FEE', 'TIP',
                             'FAILED_TRANSACTION', 'UNKNOWN_ACTIVITY', 'AMBIGUOUS_ACTIVITY')),
    status              TEXT                    NOT NULL CHECK (status IN
                            ('VERIFIED', 'PARTIALLY_DECODED', 'AMBIGUOUS', 'UNSUPPORTED', 'INSUFFICIENT_EVIDENCE', 'INVALID')),
    protocol            TEXT                    CHECK (protocol ~ '^[A-Z0-9_]{1,32}$'),
    program_id          solprint.sol_address,
    instruction_ref     TEXT                    CHECK (instruction_ref ~ '^[0-9]{1,3}(\.[0-9]{1,4})?$'),
    actor               solprint.sol_address,
    attribution         TEXT                    CHECK (attribution IN
                            ('VERIFIED_OWNER', 'VERIFIED_AUTHORITY', 'VERIFIED_BENEFICIARY', 'MULTIPLE_POSSIBLE_ACTORS',
                             'UNKNOWN_OWNER', 'PROGRAM_CONTROLLED', 'AMBIGUOUS')),
    mint                TEXT                    CHECK (mint = 'SOL' OR mint ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'),
    amount              solprint.u64_amount,
    source_account      solprint.sol_address,
    destination_account solprint.sol_address,
    counterparty        solprint.sol_address,
    reasons             TEXT[]                  NOT NULL DEFAULT '{}',
    decoder             TEXT                    NOT NULL CHECK (decoder ~ '^[a-z0-9_]{1,32}$'),
    decoder_version     TEXT                    NOT NULL CHECK (length(decoder_version) BETWEEN 1 AND 32),
    evidence_revision   INTEGER                 NOT NULL CHECK (evidence_revision >= 1),
    canonical           BOOLEAN                 NOT NULL DEFAULT TRUE,
    slot                BIGINT                  NOT NULL CHECK (slot >= 0),
    block_time          TIMESTAMPTZ,
    created_at          TIMESTAMPTZ             NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ             NOT NULL DEFAULT now(),
    FOREIGN KEY (network, signature) REFERENCES solprint.de_transactions (network, signature) ON DELETE CASCADE
);

CREATE INDEX de_activities_signature_idx ON solprint.de_activities (network, signature, activity_index);
CREATE INDEX de_activities_actor_idx ON solprint.de_activities (actor, block_time DESC) WHERE canonical AND actor IS NOT NULL;
CREATE INDEX de_activities_mint_idx ON solprint.de_activities (mint, block_time DESC) WHERE canonical AND mint IS NOT NULL;
CREATE INDEX de_activities_protocol_idx ON solprint.de_activities (protocol, block_time DESC) WHERE canonical AND protocol IS NOT NULL;

COMMENT ON TABLE solprint.de_activities IS
    'Normalized activities (several per transaction). activity_id is deterministic: reprocessing revises, never duplicates.';

-- User-level economic swaps. Route legs of an aggregated swap live in
-- de_route_legs and are never separate swaps.
CREATE TABLE solprint.de_swaps (
    swap_id               TEXT                    PRIMARY KEY CHECK (length(swap_id) <= 160),
    activity_id           TEXT                    NOT NULL UNIQUE REFERENCES solprint.de_activities (activity_id) ON DELETE CASCADE,
    network               solprint.network_id     NOT NULL,
    signature             solprint.sol_signature  NOT NULL,
    slot                  BIGINT                  NOT NULL CHECK (slot >= 0),
    block_time            TIMESTAMPTZ,
    transaction_index     INTEGER                 CHECK (transaction_index >= 0),
    wallet                solprint.sol_address,
    attribution           TEXT                    NOT NULL CHECK (attribution IN
                              ('VERIFIED_OWNER', 'VERIFIED_AUTHORITY', 'VERIFIED_BENEFICIARY', 'MULTIPLE_POSSIBLE_ACTORS',
                               'UNKNOWN_OWNER', 'PROGRAM_CONTROLLED', 'AMBIGUOUS')),
    authority             solprint.sol_address,
    beneficiary           solprint.sol_address,
    fee_payer             solprint.sol_address    NOT NULL,
    input_mint            solprint.sol_address,
    input_amount          solprint.u64_amount,
    input_decimals        SMALLINT                CHECK (input_decimals BETWEEN 0 AND 255),
    input_asset_kind      TEXT                    NOT NULL CHECK (input_asset_kind IN ('NATIVE_SOL', 'WRAPPED_SOL', 'SPL_TOKEN', 'TOKEN_2022', 'UNKNOWN')),
    output_mint           solprint.sol_address,
    output_amount         solprint.u64_amount,
    output_decimals       SMALLINT                CHECK (output_decimals BETWEEN 0 AND 255),
    output_asset_kind     TEXT                    NOT NULL CHECK (output_asset_kind IN ('NATIVE_SOL', 'WRAPPED_SOL', 'SPL_TOKEN', 'TOKEN_2022', 'UNKNOWN')),
    exchange_ratio        NUMERIC(48, 18)         CHECK (exchange_ratio > 0),
    protocol              TEXT                    NOT NULL CHECK (protocol ~ '^[A-Z0-9_]{1,32}$'),
    venue_kind            TEXT                    NOT NULL CHECK (venue_kind IN ('AGGREGATOR', 'CONSTANT_PRODUCT_AMM', 'CONCENTRATED_LIQUIDITY', 'BONDING_CURVE')),
    aggregator            TEXT                    CHECK (aggregator ~ '^[A-Z0-9_]{1,32}$'),
    route_complete        BOOLEAN                 NOT NULL,
    instruction_ref       TEXT                    NOT NULL CHECK (instruction_ref ~ '^[0-9]{1,3}(\.[0-9]{1,4})?$'),
    execution_status      TEXT                    NOT NULL CHECK (execution_status = 'SUCCESS'),
    classification_status TEXT                    NOT NULL CHECK (classification_status IN
                              ('VERIFIED', 'PARTIALLY_DECODED', 'AMBIGUOUS', 'UNSUPPORTED', 'INSUFFICIENT_EVIDENCE', 'INVALID')),
    finality              TEXT                    NOT NULL CHECK (finality IN
                              ('OBSERVED_PROCESSED', 'OBSERVED_CONFIRMED', 'FINALIZED', 'FINALITY_UNKNOWN', 'POSSIBLY_ORPHANED', 'ORPHANED')),
    reasons               TEXT[]                  NOT NULL DEFAULT '{}',
    decoder_version       TEXT                    NOT NULL CHECK (length(decoder_version) BETWEEN 1 AND 32),
    evidence_revision     INTEGER                 NOT NULL CHECK (evidence_revision >= 1),
    revision              INTEGER                 NOT NULL DEFAULT 1 CHECK (revision >= 1),
    canonical             BOOLEAN                 NOT NULL DEFAULT TRUE,
    retracted_at          TIMESTAMPTZ,
    retraction_reason     TEXT                    CHECK (retraction_reason IN ('ORPHANED', 'DECODER_REVISION', 'EVIDENCE_REVISION')),
    first_observed_at     TIMESTAMPTZ             NOT NULL,
    decoded_at            TIMESTAMPTZ             NOT NULL,
    persisted_at          TIMESTAMPTZ             NOT NULL DEFAULT now(),
    finalized_at          TIMESTAMPTZ,
    corrected_at          TIMESTAMPTZ,
    CHECK (canonical = (retracted_at IS NULL))
);

CREATE INDEX de_swaps_wallet_idx ON solprint.de_swaps (wallet, block_time DESC, swap_id)
    WHERE canonical AND classification_status = 'VERIFIED';
CREATE INDEX de_swaps_input_mint_idx ON solprint.de_swaps (input_mint, block_time DESC) WHERE canonical;
CREATE INDEX de_swaps_output_mint_idx ON solprint.de_swaps (output_mint, block_time DESC) WHERE canonical;
CREATE INDEX de_swaps_time_idx ON solprint.de_swaps (block_time DESC NULLS LAST, swap_id);
CREATE INDEX de_swaps_signature_idx ON solprint.de_swaps (network, signature);

COMMENT ON TABLE solprint.de_swaps IS
    'User-level swaps. Only canonical VERIFIED rows of successful transactions are verified trades.';

CREATE TABLE solprint.de_route_legs (
    swap_id          TEXT                  NOT NULL REFERENCES solprint.de_swaps (swap_id) ON DELETE CASCADE,
    leg_index        SMALLINT              NOT NULL CHECK (leg_index >= 0),
    protocol         TEXT                  NOT NULL CHECK (protocol ~ '^[A-Z0-9_]{1,32}$'),
    program_id       solprint.sol_address,
    pool             solprint.sol_address,
    input_mint       solprint.sol_address  NOT NULL,
    input_amount     solprint.u64_amount   NOT NULL,
    output_mint      solprint.sol_address  NOT NULL,
    output_amount    solprint.u64_amount   NOT NULL,
    instruction_ref  TEXT                  CHECK (instruction_ref ~ '^[0-9]{1,3}(\.[0-9]{1,4})?$'),
    evidence_kind    TEXT                  NOT NULL CHECK (evidence_kind IN ('PROGRAM_EVENT', 'TOKEN_TRANSFERS')),
    PRIMARY KEY (swap_id, leg_index)
);

CREATE TABLE solprint.de_swap_fees (
    swap_id     TEXT                  NOT NULL REFERENCES solprint.de_swaps (swap_id) ON DELETE CASCADE,
    fee_index   SMALLINT              NOT NULL CHECK (fee_index >= 0),
    kind        TEXT                  NOT NULL CHECK (kind IN ('NETWORK', 'PROTOCOL', 'CREATOR', 'PLATFORM', 'TRANSFER_FEE', 'OTHER')),
    mint        TEXT                  CHECK (mint = 'SOL' OR mint ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'),
    amount      solprint.u64_amount,
    provenance  TEXT                  NOT NULL CHECK (provenance IN ('EXPLICIT', 'DERIVED', 'UNKNOWN')),
    recipient   solprint.sol_address,
    PRIMARY KEY (swap_id, fee_index)
);

-- ---- tokens and prices -------------------------------------------------------------

CREATE TABLE solprint.de_tokens (
    network               solprint.network_id    NOT NULL,
    mint                  solprint.sol_address   NOT NULL,
    token_program         solprint.sol_address,
    decimals              SMALLINT               CHECK (decimals BETWEEN 0 AND 255),
    decimals_conflict     BOOLEAN                NOT NULL DEFAULT FALSE,
    first_observed_slot   BIGINT                 CHECK (first_observed_slot >= 0),
    first_observed_at     TIMESTAMPTZ            NOT NULL,
    last_observed_at      TIMESTAMPTZ            NOT NULL,
    metadata_name         TEXT                   CHECK (length(metadata_name) <= 64),
    metadata_symbol       TEXT                   CHECK (length(metadata_symbol) <= 16),
    metadata_source       TEXT                   CHECK (metadata_source IN ('JUPITER_TOKENS', 'DEX_SCREENER')),
    metadata_observed_at  TIMESTAMPTZ,
    metadata_quality      TEXT                   NOT NULL DEFAULT 'MISSING' CHECK (metadata_quality IN ('MISSING', 'PROVIDER_REPORTED', 'REJECTED_MALFORMED')),
    PRIMARY KEY (network, mint)
);

CREATE TABLE solprint.de_price_observations (
    price_id            BIGSERIAL              PRIMARY KEY,
    network             solprint.network_id    NOT NULL,
    mint                solprint.sol_address   NOT NULL,
    reference_currency  TEXT                   NOT NULL CHECK (reference_currency = 'USD'),
    price               NUMERIC(48, 18)        NOT NULL CHECK (price > 0),
    source              TEXT                   NOT NULL CHECK (source IN ('JUPITER_PRICE_V3')),
    source_slot         BIGINT                 CHECK (source_slot >= 0),
    source_timestamp    TIMESTAMPTZ,
    observed_at         TIMESTAMPTZ            NOT NULL
);

CREATE INDEX de_price_observations_mint_idx ON solprint.de_price_observations (network, mint, observed_at DESC);

CREATE TABLE solprint.de_swap_valuations (
    swap_id               TEXT                 NOT NULL REFERENCES solprint.de_swaps (swap_id) ON DELETE CASCADE,
    side                  TEXT                 NOT NULL CHECK (side IN ('INPUT', 'OUTPUT')),
    asset_mint            solprint.sol_address NOT NULL,
    reference_currency    TEXT                 NOT NULL CHECK (reference_currency = 'USD'),
    reference_price       NUMERIC(48, 18)      NOT NULL CHECK (reference_price > 0),
    price_id              BIGINT               NOT NULL REFERENCES solprint.de_price_observations (price_id),
    price_source          TEXT                 NOT NULL,
    source_timestamp      TIMESTAMPTZ,
    observation_timestamp TIMESTAMPTZ          NOT NULL,
    valuation_timestamp   TIMESTAMPTZ          NOT NULL,
    price_age_ms          BIGINT               NOT NULL CHECK (price_age_ms >= 0),
    valuation_method      TEXT                 NOT NULL CHECK (valuation_method IN ('CONTEMPORANEOUS_REFERENCE_PRICE')),
    quality_state         TEXT                 NOT NULL CHECK (quality_state IN ('WITHIN_MAX_AGE')),
    value                 NUMERIC(60, 18)      NOT NULL CHECK (value >= 0),
    PRIMARY KEY (swap_id, side)
);

-- ---- historical backfills ----------------------------------------------------------

CREATE TABLE solprint.de_backfills (
    backfill_id              UUID                  PRIMARY KEY,
    network                  solprint.network_id   NOT NULL,
    target_id                UUID                  REFERENCES solprint.de_targets (target_id) ON DELETE SET NULL,
    address                  solprint.sol_address  NOT NULL,
    commitment               TEXT                  NOT NULL CHECK (commitment IN ('confirmed', 'finalized')),
    start_before_signature   solprint.sol_signature,
    until_signature          solprint.sol_signature,
    min_slot                 BIGINT                CHECK (min_slot >= 0),
    page_size                INTEGER               NOT NULL CHECK (page_size BETWEEN 1 AND 1000),
    max_pages                INTEGER               NOT NULL CHECK (max_pages BETWEEN 1 AND 100),
    max_signatures           INTEGER               NOT NULL CHECK (max_signatures BETWEEN 1 AND 10000),
    cursor_before            solprint.sol_signature,
    pages_fetched            INTEGER               NOT NULL DEFAULT 0 CHECK (pages_fetched >= 0),
    discovered_count         INTEGER               NOT NULL DEFAULT 0 CHECK (discovered_count >= 0),
    queued_count             INTEGER               NOT NULL DEFAULT 0 CHECK (queued_count >= 0),
    duplicate_count          INTEGER               NOT NULL DEFAULT 0 CHECK (duplicate_count >= 0),
    credits_estimated        INTEGER               NOT NULL CHECK (credits_estimated >= 0),
    credits_spent            INTEGER               NOT NULL DEFAULT 0 CHECK (credits_spent >= 0),
    oldest_slot_seen         BIGINT                CHECK (oldest_slot_seen >= 0),
    newest_slot_seen         BIGINT                CHECK (newest_slot_seen >= 0),
    history_exhausted        BOOLEAN               NOT NULL DEFAULT FALSE,
    status                   TEXT                  NOT NULL CHECK (status IN
                                 ('PENDING', 'RUNNING', 'PAUSED', 'PAUSED_BY_BUDGET', 'PAUSED_BY_PROVIDER', 'PAUSED_BY_QUEUE',
                                  'COMPLETED', 'COMPLETED_WITH_GAPS', 'FAILED', 'CANCELLED')),
    stop_reason              TEXT                  CHECK (stop_reason ~ '^[A-Z0-9_]{1,64}$'),
    cancel_requested         BOOLEAN               NOT NULL DEFAULT FALSE,
    lease_owner              UUID,
    lease_expires_at         TIMESTAMPTZ,
    last_error_code          TEXT                  CHECK (last_error_code ~ '^[A-Z0-9_]{1,64}$'),
    coverage_state           TEXT                  NOT NULL DEFAULT 'NONE' CHECK (coverage_state IN
                                 ('NONE', 'PARTIAL_HISTORY', 'ADDRESS_HISTORY_EXHAUSTED', 'BOUNDARY_REACHED')),
    created_at               TIMESTAMPTZ           NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ           NOT NULL DEFAULT now(),
    started_at               TIMESTAMPTZ,
    completed_at             TIMESTAMPTZ
);

CREATE INDEX de_backfills_runnable_idx ON solprint.de_backfills (created_at)
    WHERE status IN ('PENDING', 'RUNNING', 'PAUSED_BY_BUDGET', 'PAUSED_BY_PROVIDER', 'PAUSED_BY_QUEUE');

COMMENT ON TABLE solprint.de_backfills IS
    'Bounded, resumable getSignaturesForAddress walks. The cursor advances only in the transaction that persists the page.';

-- ---- data quality ----------------------------------------------------------------------

CREATE TABLE solprint.de_quality_issues (
    issue_id         BIGSERIAL               PRIMARY KEY,
    network          solprint.network_id     NOT NULL,
    signature        solprint.sol_signature,
    target_id        UUID                    REFERENCES solprint.de_targets (target_id) ON DELETE SET NULL,
    backfill_id      UUID                    REFERENCES solprint.de_backfills (backfill_id) ON DELETE SET NULL,
    code             TEXT                    NOT NULL CHECK (code ~ '^[A-Z0-9_]{1,64}$'),
    severity         TEXT                    NOT NULL CHECK (severity IN ('INFO', 'WARNING', 'INVALID', 'OPERATIONAL', 'CRITICAL')),
    detail           TEXT                    CHECK (length(detail) <= 512),
    decoder_version  TEXT                    CHECK (length(decoder_version) <= 32),
    occurrences      INTEGER                 NOT NULL DEFAULT 1 CHECK (occurrences >= 1),
    first_seen_at    TIMESTAMPTZ             NOT NULL DEFAULT now(),
    last_seen_at     TIMESTAMPTZ             NOT NULL DEFAULT now(),
    resolved_at      TIMESTAMPTZ,
    dedup_key        TEXT                    NOT NULL CHECK (length(dedup_key) <= 256)
);

CREATE UNIQUE INDEX de_quality_issues_open_idx ON solprint.de_quality_issues (dedup_key) WHERE resolved_at IS NULL;
CREATE INDEX de_quality_issues_signature_idx ON solprint.de_quality_issues (network, signature) WHERE signature IS NOT NULL;
CREATE INDEX de_quality_issues_unresolved_idx ON solprint.de_quality_issues (severity, last_seen_at DESC) WHERE resolved_at IS NULL;

-- ---- domain event outbox ---------------------------------------------------------------

CREATE TABLE solprint.de_outbox (
    event_id            UUID                    PRIMARY KEY,
    seq                 BIGSERIAL               NOT NULL UNIQUE,
    event_type          TEXT                    NOT NULL CHECK (event_type ~ '^[A-Za-z]{1,64}$'),
    schema_version      SMALLINT                NOT NULL CHECK (schema_version >= 1),
    aggregate_type      TEXT                    NOT NULL CHECK (aggregate_type IN ('TRANSACTION', 'SWAP', 'TARGET', 'BACKFILL')),
    aggregate_id        TEXT                    NOT NULL CHECK (length(aggregate_id) <= 160),
    aggregate_revision  INTEGER                 NOT NULL CHECK (aggregate_revision >= 1),
    network             solprint.network_id     NOT NULL,
    source              TEXT                    NOT NULL CHECK (source IN ('HELIUS', 'SOLANA_RPC', 'INTERNAL', 'FIXTURE')),
    source_event_time   TIMESTAMPTZ,
    observed_at         TIMESTAMPTZ             NOT NULL,
    persisted_at        TIMESTAMPTZ             NOT NULL DEFAULT now(),
    correlation_id      UUID                    NOT NULL,
    causation_id        UUID,
    commitment          TEXT                    CHECK (commitment IN ('confirmed', 'finalized')),
    quality_state       TEXT                    NOT NULL CHECK (quality_state IN
                            ('VERIFIED', 'PARTIALLY_DECODED', 'AMBIGUOUS', 'UNSUPPORTED', 'INSUFFICIENT_EVIDENCE', 'INVALID', 'NOT_APPLICABLE')),
    payload             JSONB                   NOT NULL CHECK (octet_length(payload::text) <= 16384),
    -- Re-processing the same revision never publishes a second copy.
    UNIQUE (aggregate_type, aggregate_id, aggregate_revision, event_type)
);

CREATE INDEX de_outbox_aggregate_idx ON solprint.de_outbox (aggregate_type, aggregate_id, aggregate_revision);
CREATE INDEX de_outbox_persisted_idx ON solprint.de_outbox (persisted_at);

CREATE TABLE solprint.de_outbox_deliveries (
    consumer         TEXT        NOT NULL CHECK (consumer ~ '^[a-z0-9_.]{1,64}$'),
    event_id         UUID        NOT NULL REFERENCES solprint.de_outbox (event_id) ON DELETE CASCADE,
    status           TEXT        NOT NULL CHECK (status IN ('DELIVERED', 'FAILED')),
    attempts         INTEGER     NOT NULL DEFAULT 1 CHECK (attempts >= 1),
    last_error_code  TEXT        CHECK (last_error_code ~ '^[A-Z0-9_]{1,64}$'),
    next_attempt_at  TIMESTAMPTZ,
    delivered_at     TIMESTAMPTZ,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (consumer, event_id),
    CHECK ((status = 'DELIVERED') = (delivered_at IS NOT NULL))
);

COMMENT ON TABLE solprint.de_outbox IS
    'Transactional outbox: domain events inserted in the same database transaction as the records they describe. At-least-once delivery.';
