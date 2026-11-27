/**
 * Synthetic DATA ENGINE responses for UI and visual tests only. Addresses and
 * signatures are labelled placeholders (valid base58 shape, not real
 * accounts); amounts exercise formatting and layout, they are not
 * observations and never appear in the application.
 */
import type {
  DataBackfillsResponse,
  DataDecodersResponse,
  DataEngineStatus,
  DataEventsResponse,
  DataIssuesResponse,
  DataSwapsResponse,
  DataTargetsResponse,
  DataTransactionDetail,
  DataTransactionsResponse,
  SwapDto,
  TransactionSummaryDto,
} from '../../src/types/api.ts';

const AT = '2026-11-26T12:00:00.000Z';
const WSOL = 'So11111111111111111111111111111111111111112';
const USDC = 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v';

export type DataVariant = 'active' | 'empty' | 'partial';

/** 88-character base58 placeholder signature. */
export const sig = (i: number) => `5ynthFixtureSig${i.toString().padStart(3, '1')}`.padEnd(88, 'K');
/** 44-character base58 placeholder address. */
const addr = (label: string) => `${label}Fixture`.padEnd(44, '1');

const MEME = addr('MemeMint');
const WALLET = addr('Trader');
const RELAYER = addr('Relayer');

const at = (minutesAgo: number) =>
  new Date(Date.parse(AT) - minutesAgo * 60_000).toISOString().replace(/\.\d{3}Z$/, '.000Z');

function swap(i: number, over: Partial<SwapDto>): SwapDto {
  return {
    swap_id: `swap:${sig(i)}:2`,
    signature: sig(i),
    slot: 330_000_000 - i * 37,
    block_time: at(i * 3),
    wallet: WALLET,
    attribution: 'VERIFIED_OWNER',
    authority: WALLET,
    beneficiary: null,
    fee_payer: WALLET,
    input: { mint: WSOL, amount: '500000000', decimals: 9, asset_kind: 'WRAPPED_SOL' },
    output: { mint: MEME, amount: '17000000000000', decimals: 6, asset_kind: 'SPL_TOKEN' },
    exchange_ratio: '34000.000000000000000000',
    protocol: 'RAYDIUM_AMM_V4',
    venue_kind: 'CONSTANT_PRODUCT_AMM',
    aggregator: null,
    route_complete: true,
    instruction_ref: '2',
    classification_status: 'VERIFIED',
    reasons: [],
    finality: 'FINALIZED',
    canonical: true,
    retraction_reason: null,
    revision: 1,
    decoder_version: 'de-2026.11.1',
    legs: [],
    fees: [],
    valuation: {
      state: 'NOT_ENRICHED',
      usd_value: null,
      price_source: null,
      price_observed_at: null,
    },
    first_observed_at: at(i * 3),
    decoded_at: at(i * 3),
    finalized_at: at(i * 3 - 1),
    corrected_at: null,
    ...over,
  };
}

const swaps: SwapDto[] = [
  swap(1, {}),
  swap(2, {
    protocol: 'JUPITER',
    venue_kind: 'AGGREGATOR',
    aggregator: 'JUPITER',
    input: { mint: USDC, amount: '1250000000', decimals: 6, asset_kind: 'SPL_TOKEN' },
    output: { mint: WSOL, amount: '8123456789', decimals: 9, asset_kind: 'WRAPPED_SOL' },
    exchange_ratio: '0.006498765431200000',
    finality: 'OBSERVED_CONFIRMED',
    finalized_at: null,
    legs: [
      {
        index: 0,
        protocol: 'ORCA_WHIRLPOOL',
        program_id: addr('OrcaProgram'),
        pool: addr('PoolA'),
        input_mint: USDC,
        input_amount: '1250000000',
        output_mint: MEME,
        output_amount: '40000000000',
        instruction_ref: '3.1',
        evidence: 'PROGRAM_EVENT',
      },
      {
        index: 1,
        protocol: 'RAYDIUM_CLMM',
        program_id: addr('ClmmProgram'),
        pool: addr('PoolB'),
        input_mint: MEME,
        input_amount: '40000000000',
        output_mint: WSOL,
        output_amount: '8123456789',
        instruction_ref: '3.4',
        evidence: 'PROGRAM_EVENT',
      },
    ],
    fees: [
      {
        kind: 'PLATFORM',
        mint: WSOL,
        amount: '812345',
        provenance: 'EXPLICIT',
        recipient: addr('FeeAcct'),
      },
    ],
  }),
  swap(3, {
    protocol: 'PUMP_FUN',
    venue_kind: 'BONDING_CURVE',
    exchange_ratio: '35612.003300000000000000',
    output: { mint: MEME, amount: '17806001650000', decimals: 6, asset_kind: 'SPL_TOKEN' },
    reasons: ['GROSS_OF_EXPLICIT_FEES'],
    fees: [
      { kind: 'PROTOCOL', mint: WSOL, amount: '4750000', provenance: 'EXPLICIT', recipient: null },
      { kind: 'CREATOR', mint: WSOL, amount: '250000', provenance: 'EXPLICIT', recipient: null },
    ],
  }),
  swap(4, {
    protocol: 'PUMPSWAP',
    attribution: 'VERIFIED_AUTHORITY',
    fee_payer: RELAYER,
    input: { mint: MEME, amount: '2500000000000', decimals: 6, asset_kind: 'SPL_TOKEN' },
    output: { mint: WSOL, amount: '71234567', decimals: 9, asset_kind: 'WRAPPED_SOL' },
    exchange_ratio: '0.000028493826800000',
  }),
  swap(5, {
    protocol: 'ORCA_WHIRLPOOL',
    venue_kind: 'CONCENTRATED_LIQUIDITY',
    wallet: null,
    attribution: 'PROGRAM_CONTROLLED',
    classification_status: 'AMBIGUOUS',
    reasons: ['AUTHORITY_NOT_SIGNER'],
    finality: 'FINALITY_UNKNOWN',
    finalized_at: null,
  }),
  swap(6, {
    protocol: 'RAYDIUM_CPMM',
    input: {
      mint: addr('Token2022Mint'),
      amount: '990000000',
      decimals: 9,
      asset_kind: 'TOKEN_2022',
    },
    output: { mint: USDC, amount: '15004211', decimals: 6, asset_kind: 'SPL_TOKEN' },
    exchange_ratio: '0.015155768686868686',
    fees: [
      {
        kind: 'TRANSFER_FEE',
        mint: addr('Token2022Mint'),
        amount: '10000000',
        provenance: 'EXPLICIT',
        recipient: null,
      },
    ],
  }),
  swap(7, {
    protocol: 'RAYDIUM_CLMM',
    venue_kind: 'CONCENTRATED_LIQUIDITY',
    classification_status: 'INSUFFICIENT_EVIDENCE',
    output: { mint: MEME, amount: null, decimals: 6, asset_kind: 'SPL_TOKEN' },
    exchange_ratio: null,
    reasons: ['INNER_INSTRUCTIONS_MISSING'],
    finality: 'POSSIBLY_ORPHANED',
    finalized_at: null,
  }),
  swap(8, {
    canonical: false,
    retraction_reason: 'DECODER_OR_EVIDENCE_REVISION',
    revision: 2,
  }),
];

function tx(i: number, over: Partial<TransactionSummaryDto> = {}): TransactionSummaryDto {
  const s = swaps[i - 1];
  return {
    signature: sig(i),
    slot: 330_000_000 - i * 37,
    block_time: at(i * 3),
    execution_status: 'SUCCESS',
    fee_lamports: '105000',
    fee_payer: WALLET,
    finality: s?.finality ?? 'FINALIZED',
    decode_status: s?.classification_status ?? 'VERIFIED',
    decoder_version: 'de-2026.11.1',
    protocols: s ? [s.protocol] : [],
    activity_kinds: ['FEE', 'SWAP'],
    swap_count: s ? 1 : 0,
    revision: 1,
    first_observed_at: at(i * 3),
    decoded_at: at(i * 3),
    ...over,
  };
}

const txs: TransactionSummaryDto[] = [
  ...[1, 2, 3, 4, 5, 6, 7].map((i) => tx(i)),
  tx(9, {
    execution_status: 'FAILED',
    decode_status: 'VERIFIED',
    protocols: [],
    activity_kinds: ['FEE', 'FAILED_TRANSACTION'],
    swap_count: 0,
  }),
  tx(10, {
    decode_status: 'UNSUPPORTED',
    protocols: [],
    swap_count: 0,
    activity_kinds: ['FEE', 'UNKNOWN_ACTIVITY'],
  }),
];

export function dataStatus(v: DataVariant): DataEngineStatus {
  const base = {
    running: true,
    execution_capability: 'NONE' as const,
    decoder_version: 'de-2026.11.1',
    generated_at: AT,
    settings: {
      auto_ingest_enabled: false,
      backfill_enabled: true,
      price_enrichment_enabled: false,
      default_commitment: 'confirmed',
      fetch_concurrency: 4,
      max_active_targets: 25,
    },
  };
  if (v === 'partial') {
    return {
      ...base,
      counts_at: null,
      counts_available: false,
      queue: null,
      transactions: null,
      swaps: null,
      open_issues_by_severity: [],
      outbox: null,
      evidence: null,
      targets_enabled: null,
      targets_total: null,
      backfills_active: null,
      // The engine is running (only the database is down), so in-process
      // counters are still reported, exactly as the backend does.
      process: {
        jobs_claimed: 6,
        fetched: 0,
        fetched_cached: 0,
        persisted: 0,
        unchanged: 0,
        retries: 6,
        unavailable: 0,
        quarantined: 0,
        budget_pauses: 0,
        credits_charged: 0,
        finality_checks: 0,
        stream_observations: 0,
        stream_gaps: 0,
        backfill_pages: 0,
        outbox_delivered: 0,
        mean_decode_us: null,
        last_error_code: 'JOB_ERROR',
        last_error_at: at(1),
      },
    };
  }
  const empty = v === 'empty';
  return {
    ...base,
    counts_at: AT,
    counts_available: true,
    queue: {
      depth: empty ? 0 : 42,
      capacity: 2048,
      by_state: empty
        ? []
        : [
            { label: 'PENDING', count: 38 },
            { label: 'FETCHING', count: 4 },
            { label: 'COMPLETED', count: 1287 },
            { label: 'UNAVAILABLE', count: 3 },
            { label: 'QUARANTINED', count: 1 },
          ],
      oldest_pending_at: empty ? null : at(6),
    },
    transactions: {
      total: empty ? 0 : 1287,
      by_finality: empty
        ? []
        : [
            { label: 'FINALIZED', count: 1201 },
            { label: 'OBSERVED_CONFIRMED', count: 79 },
            { label: 'FINALITY_UNKNOWN', count: 5 },
            { label: 'POSSIBLY_ORPHANED', count: 2 },
          ],
      by_decode_status: empty
        ? []
        : [
            { label: 'VERIFIED', count: 1104 },
            { label: 'PARTIALLY_DECODED', count: 61 },
            { label: 'UNSUPPORTED', count: 97 },
            { label: 'INSUFFICIENT_EVIDENCE', count: 25 },
          ],
      last_decoded_at: empty ? null : at(0),
    },
    swaps: {
      canonical: empty ? 0 : 913,
      verified: empty ? 0 : 861,
      retracted: empty ? 0 : 6,
      by_protocol: empty ? [] : [{ label: 'JUPITER', count: 402 }],
    },
    open_issues_by_severity: empty
      ? []
      : [
          { label: 'WARNING', count: 31 },
          { label: 'OPERATIONAL', count: 4 },
          { label: 'INVALID', count: 1 },
        ],
    outbox: { events: empty ? 0 : 2206, undelivered: empty ? 0 : 3 },
    evidence: { bytes: empty ? 0 : 41_822_310, pruned: 0, retention_days: 90 },
    targets_enabled: empty ? 0 : 2,
    targets_total: empty ? 0 : 3,
    backfills_active: empty ? 0 : 1,
    process: {
      jobs_claimed: empty ? 0 : 1302,
      fetched: empty ? 0 : 1296,
      fetched_cached: empty ? 0 : 9,
      persisted: empty ? 0 : 1287,
      unchanged: empty ? 0 : 14,
      retries: empty ? 0 : 22,
      unavailable: empty ? 0 : 3,
      quarantined: empty ? 0 : 1,
      budget_pauses: 0,
      credits_charged: empty ? 0 : 1338,
      finality_checks: empty ? 0 : 4120,
      stream_observations: 0,
      stream_gaps: 0,
      backfill_pages: empty ? 0 : 12,
      outbox_delivered: empty ? 0 : 2203,
      mean_decode_us: empty ? null : 184,
      last_error_code: empty ? null : 'RATE_LIMITED',
      last_error_at: empty ? null : at(18),
    },
  };
}

export function dataSwaps(v: DataVariant, protocol: string | null): DataSwapsResponse {
  const items = v === 'active' ? swaps.filter((s) => !protocol || s.protocol === protocol) : [];
  return { generated_at: AT, items, next_cursor: null };
}

export function dataTransactions(v: DataVariant): DataTransactionsResponse {
  return { generated_at: AT, items: v === 'active' ? txs : [], next_cursor: null };
}

export function dataTransaction(signature: string): DataTransactionDetail | null {
  const i = txs.findIndex((t) => t.signature === signature);
  const t = txs[i];
  if (!t) return null;
  const s = swaps.find((x) => x.signature === signature);
  return {
    generated_at: AT,
    summary: t,
    transaction_index: 812,
    version: '0',
    err_json: null,
    signers: [WALLET],
    compute_units: '84213',
    finality_checks: 3,
    finalized_at: s?.finalized_at ?? null,
    evidence: [
      {
        revision: 1,
        commitment: 'confirmed',
        provider: 'HELIUS',
        slot: t.slot,
        fetched_at: t.first_observed_at,
        content_sha256: 'a3f1c09e5b7d2e4f6a8b0c1d3e5f7a9b1c3d5e7f9a1b3c5d7e9f1a3b5c7d9e1f',
        byte_len: 14_208,
        pruned: false,
        canonical: true,
        superseded_reason: null,
      },
    ],
    balances: [
      {
        account_index: 0,
        asset: 'SOL',
        account: WALLET,
        owner: null,
        token_program: null,
        decimals: 9,
        pre_amount: '2500000000',
        post_amount: '1997895000',
        delta: '-502105000',
        pre_evidence: 'REPORTED',
        post_evidence: 'REPORTED',
        ownership_evidence: 'NATIVE_ACCOUNT',
      },
      {
        account_index: 3,
        asset: MEME,
        account: addr('TraderMemeAta'),
        owner: WALLET,
        token_program: addr('TokenProgram'),
        decimals: 6,
        pre_amount: '0',
        post_amount: '17000000000000',
        delta: '17000000000000',
        pre_evidence: 'INFERRED_CREATED',
        post_evidence: 'REPORTED',
        ownership_evidence: 'TOKEN_BALANCE_META',
      },
    ],
    activities: [
      {
        activity_id: `${signature}:tx:FEE:0`,
        index: 0,
        kind: 'FEE',
        status: 'VERIFIED',
        protocol: null,
        program_id: null,
        instruction_ref: null,
        actor: WALLET,
        attribution: 'VERIFIED_OWNER',
        mint: null,
        amount: '105000',
        source_account: WALLET,
        destination_account: null,
        counterparty: null,
        reasons: [],
        decoder: 'transaction',
        canonical: true,
      },
      {
        activity_id: `${signature}:2:SWAP:1`,
        index: 1,
        kind: 'SWAP',
        status: s?.classification_status ?? 'VERIFIED',
        protocol: s?.protocol ?? null,
        program_id: addr('DexProgram'),
        instruction_ref: '2',
        actor: WALLET,
        attribution: 'VERIFIED_OWNER',
        mint: null,
        amount: null,
        source_account: null,
        destination_account: null,
        counterparty: null,
        reasons: [],
        decoder: 'raydium_amm_v4',
        canonical: true,
      },
    ],
    swaps: s ? [s] : [],
    issues: [],
  };
}

export function dataIssues(v: DataVariant): DataIssuesResponse {
  return {
    generated_at: AT,
    items:
      v === 'active'
        ? [
            {
              issue_id: 91,
              signature: sig(7),
              target_id: null,
              backfill_id: null,
              code: 'INNER_INSTRUCTIONS_MISSING',
              severity: 'WARNING',
              detail: 'meta.innerInstructions absent; amounts not inferred',
              decoder_version: 'de-2026.11.1',
              occurrences: 1,
              first_seen_at: at(21),
              last_seen_at: at(21),
              resolved_at: null,
            },
            {
              issue_id: 88,
              signature: null,
              target_id: null,
              backfill_id: null,
              code: 'TRANSACTION_UNAVAILABLE',
              severity: 'OPERATIONAL',
              detail: 'not returned at required commitment',
              decoder_version: 'de-2026.11.1',
              occurrences: 3,
              first_seen_at: at(55),
              last_seen_at: at(12),
              resolved_at: null,
            },
            {
              issue_id: 80,
              signature: sig(5),
              target_id: null,
              backfill_id: null,
              code: 'DECODER_AMOUNT_MISMATCH',
              severity: 'INVALID',
              detail: 'transfer total differs from balance delta by 1 base unit',
              decoder_version: 'de-2026.11.1',
              occurrences: 1,
              first_seen_at: at(90),
              last_seen_at: at(90),
              resolved_at: null,
            },
          ]
        : [],
  };
}

export function dataTargets(v: DataVariant): DataTargetsResponse {
  return {
    generated_at: AT,
    max_active_targets: 25,
    items:
      v === 'active'
        ? [
            {
              target_id: '0b6f9a52-5f0c-4c55-8d5e-3e1f0f8a6b11',
              target_type: 'WALLET_ADDRESS',
              address: WALLET,
              label: 'fixture-a',
              enabled: true,
              priority: 'P3',
              commitment: 'confirmed',
              discovery_source: 'MANUAL',
              created_at: at(600),
              last_observation_at: at(2),
              history_oldest_slot: 329_100_000,
              history_newest_slot: 330_000_000,
              history_exhausted: false,
              coverage_state: 'PARTIAL_HISTORY',
            },
            {
              target_id: '5d2c7e04-91a3-4d7a-b6f2-8c4e1a9d3f22',
              target_type: 'WALLET_ADDRESS',
              address: addr('Second'),
              label: null,
              enabled: true,
              priority: 'P4',
              commitment: 'finalized',
              discovery_source: 'MANUAL',
              created_at: at(400),
              last_observation_at: null,
              history_oldest_slot: null,
              history_newest_slot: null,
              history_exhausted: true,
              coverage_state: 'ADDRESS_HISTORY_EXHAUSTED',
            },
            {
              target_id: '9e1a4b37-2c6d-4f8e-a0b5-7d3c9e2f1a33',
              target_type: 'WALLET_ADDRESS',
              address: addr('Paused'),
              label: 'disabled',
              enabled: false,
              priority: 'P5',
              commitment: 'confirmed',
              discovery_source: 'MANUAL',
              created_at: at(300),
              last_observation_at: null,
              history_oldest_slot: null,
              history_newest_slot: null,
              history_exhausted: false,
              coverage_state: 'NONE',
            },
          ]
        : [],
  };
}

export function dataBackfills(v: DataVariant): DataBackfillsResponse {
  return {
    generated_at: AT,
    items:
      v === 'active'
        ? [
            {
              backfill_id: '3f8b2d1e-6a4c-4e9f-b7d2-1c5a8e3f9b44',
              target_id: '0b6f9a52-5f0c-4c55-8d5e-3e1f0f8a6b11',
              address: WALLET,
              commitment: 'confirmed',
              status: 'PAUSED_BY_BUDGET',
              stop_reason: 'BUDGET_DENIED',
              coverage_state: 'PARTIAL_HISTORY',
              cancel_requested: false,
              pages_fetched: 4,
              max_pages: 10,
              discovered_count: 400,
              queued_count: 361,
              duplicate_count: 39,
              max_signatures: 1000,
              credits_estimated: 10,
              credits_spent: 4,
              min_slot: null,
              oldest_slot_seen: 329_100_000,
              newest_slot_seen: 330_000_000,
              history_exhausted: false,
              last_error_code: null,
              created_at: at(120),
              updated_at: at(30),
              started_at: at(118),
              completed_at: null,
            },
            {
              backfill_id: '7a1c5e9b-3d2f-4b8a-9e6c-2f4d8b1a7c55',
              target_id: null,
              address: addr('Second'),
              commitment: 'finalized',
              status: 'COMPLETED',
              stop_reason: 'HISTORY_EXHAUSTED',
              coverage_state: 'ADDRESS_HISTORY_EXHAUSTED',
              cancel_requested: false,
              pages_fetched: 2,
              max_pages: 10,
              discovered_count: 137,
              queued_count: 137,
              duplicate_count: 0,
              max_signatures: 1000,
              credits_estimated: 10,
              credits_spent: 2,
              min_slot: null,
              oldest_slot_seen: 318_000_000,
              newest_slot_seen: 329_990_000,
              history_exhausted: true,
              last_error_code: null,
              created_at: at(400),
              updated_at: at(390),
              started_at: at(399),
              completed_at: at(390),
            },
          ]
        : [],
  };
}

export function dataEvents(v: DataVariant): DataEventsResponse {
  const types = ['SwapRecorded', 'TransactionDecoded', 'TransactionFinalized', 'SwapRetracted'];
  return {
    generated_at: AT,
    items:
      v === 'active'
        ? types.map((t, i) => ({
            event_id: `c0ffee${i.toString()}0-1111-4222-8333-444455556666`,
            seq: 2206 - i,
            event_type: t,
            aggregate_type: t.startsWith('Swap') ? 'SWAP' : 'TRANSACTION',
            aggregate_id: t.startsWith('Swap') ? `swap:${sig(i + 1)}:2` : sig(i + 1),
            aggregate_revision: t === 'SwapRetracted' ? 2 : 1,
            quality_state: t === 'SwapRetracted' ? 'NOT_APPLICABLE' : 'VERIFIED',
            persisted_at: at(i),
          }))
        : [],
  };
}

/** The registry is static and needs no database, so every variant returns it. */
export function dataDecoders(): DataDecodersResponse {
  const d = (id: string, protocol: string, program_id: string) => ({ id, protocol, program_id });
  return {
    decoder_version: 'de-2026.11.1',
    network: 'solana-mainnet',
    decoders: [
      d('jupiter_v6', 'JUPITER', 'JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4'),
      d('pump_fun', 'PUMP_FUN', '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'),
      d('pumpswap', 'PUMPSWAP', 'pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA'),
      d('raydium_amm_v4', 'RAYDIUM_AMM_V4', '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8'),
      d('raydium_cpmm', 'RAYDIUM_CPMM', 'CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C'),
      d('raydium_clmm', 'RAYDIUM_CLMM', 'CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK'),
      d('orca_whirlpool', 'ORCA_WHIRLPOOL', 'whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc'),
    ],
  };
}
