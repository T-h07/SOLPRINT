/**
 * Synthetic RADAR responses for UI and visual tests only. Addresses are
 * labelled placeholders (valid base58 shape, not real accounts); scores,
 * counts and amounts exercise formatting and layout. They are not
 * observations, say nothing about any real wallet and never appear in the
 * application.
 */
import type {
  RadarCyclesResponse,
  RadarEvidenceResponse,
  RadarHistoryResponse,
  RadarJobsResponse,
  RadarMarketsResponse,
  RadarMonitoringResponse,
  RadarSourcesResponse,
  RadarStatus,
  RadarWalletDetail,
  RadarWalletDto,
  RadarWalletsResponse,
} from '../../src/types/api.ts';

const AT = '2026-11-26T12:00:00.000Z';
const at = (minutesAgo: number) =>
  new Date(Date.parse(AT) - minutesAgo * 60_000).toISOString().replace(/\.\d{3}Z$/, '.000Z');

/** 44-character base58 placeholder address; the label also ends it so shortened ids differ. */
export const radarAddr = (label: string) =>
  `${label}Fixture`.padEnd(40, '1') + label.slice(-4).padStart(4, '1');

export type RadarVariant = 'active' | 'empty' | 'paused';

const settings: RadarStatus['settings'] = {
  market_discovery_interval_secs: 300,
  max_market_samples_per_cycle: 5,
  max_concurrent_sample_jobs: 2,
  max_concurrent_history_jobs: 2,
  max_investigating_candidates: 25,
  max_live_monitored_wallets: 10,
  initial_sample_signatures: 25,
  history_stages: [
    { stage: 'A', days: 7, signatures: 50 },
    { stage: 'B', days: 30, signatures: 200 },
    { stage: 'C', days: 90, signatures: 500 },
  ],
  bucket_shares_bps: [
    { label: 'ESTABLISHED', count: 3500 },
    { label: 'EMERGING', count: 2500 },
    { label: 'LOCAL_ACTIVITY', count: 2000 },
    { label: 'HISTORICAL', count: 1000 },
    { label: 'EXPLORATION', count: 1000 },
  ],
  dormant_after_days: 14,
  archive_after_days: 60,
  classification_ruleset: 'radar-class-1',
  priority_policy: 'radar-priority-1',
  lifecycle_policy: 'radar-lifecycle-1',
};

export function radarStatus(v: RadarVariant): RadarStatus {
  const empty = v === 'empty';
  return {
    enabled: true,
    running: true,
    auto_discovery_enabled: !empty,
    data_engine_running: true,
    live_monitoring_available: false,
    paused: v === 'paused',
    pause_reason: v === 'paused' ? 'OPERATOR_PAUSE' : null,
    cycle_requested: false,
    budget:
      v === 'paused'
        ? {
            gateway_state: 'CONSERVATION',
            ledger_loaded: true,
            mode: 'CONSERVATION',
            reason: 'BUDGET_CONSERVATION',
            share_bps: 4000,
            allowance: 61_200,
            available: 60_940,
            outstanding_estimates: 260,
            spent_24h: 4_812,
            spent_24h_by_class: [
              { label: 'TRANSACTION_SAMPLING', count: 1_302 },
              { label: 'INITIAL_HISTORY', count: 3_510 },
            ],
          }
        : empty
          ? {
              gateway_state: 'NORMAL',
              ledger_loaded: true,
              mode: 'FULL',
              reason: null,
              share_bps: 4000,
              allowance: 3_840_000,
              available: 3_840_000,
              outstanding_estimates: 0,
              spent_24h: 0,
              spent_24h_by_class: [],
            }
          : {
              gateway_state: 'NORMAL',
              ledger_loaded: true,
              mode: 'FULL',
              reason: null,
              share_bps: 4000,
              allowance: 3_612_480,
              available: 3_611_958,
              outstanding_estimates: 522,
              spent_24h: 18_245,
              spent_24h_by_class: [
                { label: 'TRANSACTION_SAMPLING', count: 2_904 },
                { label: 'INITIAL_HISTORY', count: 6_118 },
                { label: 'EXPANDED_HISTORY', count: 5_870 },
                { label: 'EXTENDED_HISTORY', count: 3_353 },
              ],
            },
    registry: empty
      ? {
          wallets: 0,
          wallets_new_24h: 0,
          by_lifecycle: [],
          by_eligibility: [],
          evidence: 0,
          evidence_invalidated: 0,
          reactivations: 0,
        }
      : {
          wallets: 1_284,
          wallets_new_24h: 96,
          by_lifecycle: [
            { label: 'DISCOVERED', count: 212 },
            { label: 'SCREENING', count: 37 },
            { label: 'HISTORY_QUEUED', count: 18 },
            { label: 'INVESTIGATING', count: 2 },
            { label: 'PRELIMINARY_QUALIFIED', count: 9 },
            { label: 'PRIORITIZED', count: 4 },
            { label: 'MONITORED', count: 6 },
            { label: 'INSUFFICIENT_EVIDENCE', count: 811 },
            { label: 'DORMANT', count: 121 },
            { label: 'REJECTED', count: 58 },
            { label: 'ARCHIVED', count: 6 },
          ],
          by_eligibility: [
            { label: 'ELIGIBLE_CANDIDATE', count: 402 },
            { label: 'NEEDS_MORE_EVIDENCE', count: 820 },
            { label: 'KNOWN_INFRASTRUCTURE', count: 41 },
          ],
          evidence: 9_731,
          evidence_invalidated: 14,
          reactivations: 3,
        },
    markets: empty
      ? { markets: 0, by_selection: [], pools: 0, pools_supported: 0 }
      : {
          markets: 64,
          by_selection: [
            { label: 'SELECTED', count: 5 },
            { label: 'CANDIDATE', count: 41 },
            { label: 'EXCLUDED', count: 18 },
          ],
          pools: 92,
          pools_supported: 71,
        },
    jobs: empty
      ? {
          open: 0,
          by_state: [],
          open_by_class: [],
          deferred_by_reason: [],
          oldest_pending_at: null,
        }
      : {
          open: 23,
          by_state: [
            { label: 'PENDING', count: 17 },
            { label: 'WAITING_DATA', count: 4 },
            { label: 'DEFERRED', count: 2 },
            { label: 'COMPLETED', count: 1_903 },
          ],
          open_by_class: [
            { label: 'INITIAL_HISTORY', count: 18 },
            { label: 'TRANSACTION_SAMPLING', count: 5 },
          ],
          deferred_by_reason: [{ label: 'INVESTIGATION_CAPACITY', count: 2 }],
          oldest_pending_at: at(41),
        },
    monitoring_active: empty ? [] : [{ label: 'PERIODIC_HISTORY_REFRESH', count: 6 }],
    last_cycle: empty
      ? null
      : {
          cycle_id: '0195f0a1-0000-7000-8000-000000000001',
          trigger: 'SCHEDULED',
          status: v === 'paused' ? 'DEFERRED' : 'COMPLETED',
          budget_state: v === 'paused' ? 'CONSERVATION' : 'NORMAL',
          deferred_reason: v === 'paused' ? 'OPERATOR_PAUSE' : null,
          observations: 120,
          markets_new: 3,
          markets_selected: 5,
          samples_scheduled: 5,
          credits_estimated: 130,
          started_at: at(2),
          finished_at: at(2),
        },
    last_completed_cycle_at: empty ? null : at(2),
    cycles_24h: empty ? 0 : 288,
    process: {
      cycles: empty ? 0 : 34,
      cycles_deferred: 0,
      source_failures: empty ? 0 : 1,
      provider_requests: empty ? 0 : 612,
      events_processed: empty ? 0 : 8_214,
      event_failures: 0,
      evidence_recorded: empty ? 0 : 8_190,
      evidence_invalidated: empty ? 0 : 4,
      wallets_discovered: empty ? 0 : 96,
      evaluations: empty ? 0 : 2_311,
      evaluation_conflicts: 0,
      jobs_started: empty ? 0 : 214,
      jobs_completed: empty ? 0 : 207,
      jobs_deferred: empty ? 0 : 9,
      monitoring_changes: empty ? 0 : 7,
      errors: 0,
      last_cycle_at: empty ? null : at(2),
      last_event_at: empty ? null : at(0),
      last_evaluation_at: empty ? null : at(0),
      last_error_code: null,
      last_error_at: null,
    },
    settings,
    timestamp: AT,
  };
}

function wallet(i: number, over: Partial<RadarWalletDto>): RadarWalletDto {
  return {
    address: radarAddr(`Wa11et${String(i)}`),
    lifecycle: 'INSUFFICIENT_EVIDENCE',
    eligibility: 'NEEDS_MORE_EVIDENCE',
    eligibility_reasons: [],
    qualification: 'INSUFFICIENT_ACTIVITY',
    qualification_reasons: [],
    priority_score: '12.4000',
    priority_policy: 'radar-priority-1',
    history_stage: 'A',
    coverage_state: 'REQUESTED_SCOPE_COMPLETE',
    coverage_start: at(7 * 24 * 60),
    coverage_end: at(30),
    monitoring_mode: 'INACTIVE',
    first_discovery_source: 'ESTABLISHED',
    first_discovered_at: at(600 + i * 13),
    last_verified_trade_at: at(30 + i * 47),
    last_investigated_at: at(40),
    last_evaluated_at: at(1),
    next_investigation_at: null,
    evidence: 3,
    valid_evidence: 3,
    verified_swaps: 3,
    distinct_tokens: 1,
    operator_excluded: false,
    revision: 7,
    ...over,
  };
}

export const TRADER = radarAddr('Wa11et1');

const WALLETS: RadarWalletDto[] = [
  wallet(1, {
    lifecycle: 'MONITORED',
    eligibility: 'ELIGIBLE_CANDIDATE',
    qualification: 'READY_FOR_DEEPER_ANALYSIS',
    priority_score: '71.3824',
    history_stage: 'C',
    monitoring_mode: 'PERIODIC_HISTORY_REFRESH',
    verified_swaps: 412,
    distinct_tokens: 38,
  }),
  wallet(2, {
    lifecycle: 'PRIORITIZED',
    eligibility: 'ELIGIBLE_CANDIDATE',
    qualification: 'READY_FOR_DEEPER_ANALYSIS',
    priority_score: '64.0051',
    history_stage: 'C',
    coverage_state: 'PARTIAL_HISTORY',
    verified_swaps: 198,
    distinct_tokens: 21,
  }),
  wallet(3, {
    lifecycle: 'PRELIMINARY_QUALIFIED',
    eligibility: 'ELIGIBLE_CANDIDATE',
    qualification: 'PRELIMINARY_POSITIVE_RETURN',
    priority_score: '52.7700',
    history_stage: 'B',
    coverage_state: 'INVESTIGATION_PENDING',
    verified_swaps: 87,
    distinct_tokens: 12,
  }),
  wallet(4, {
    lifecycle: 'INVESTIGATING',
    eligibility: 'PROVISIONAL_CANDIDATE',
    qualification: null,
    priority_score: '40.1000',
    history_stage: 'A',
    coverage_state: 'INVESTIGATION_PENDING',
    first_discovery_source: 'EMERGING',
    verified_swaps: 14,
    distinct_tokens: 4,
  }),
  wallet(5, {
    lifecycle: 'DORMANT',
    eligibility: 'ELIGIBLE_CANDIDATE',
    qualification: 'INSUFFICIENT_ACTIVITY',
    priority_score: '22.9000',
    history_stage: 'B',
    last_verified_trade_at: at(19 * 24 * 60),
    verified_swaps: 33,
    distinct_tokens: 6,
  }),
  wallet(6, {
    lifecycle: 'REJECTED',
    eligibility: 'ELIGIBLE_CANDIDATE',
    qualification: 'PRELIMINARY_NEGATIVE_RETURN',
    priority_score: '18.0000',
    history_stage: 'B',
    verified_swaps: 41,
    distinct_tokens: 9,
  }),
  wallet(7, {
    coverage_state: 'DECODER_COVERAGE_LIMITED',
    first_discovery_source: 'LOCAL_ACTIVITY',
  }),
  wallet(8, {
    lifecycle: 'DISCOVERED',
    qualification: null,
    priority_score: null,
    history_stage: 'NONE',
    coverage_state: 'NOT_INVESTIGATED',
    coverage_start: null,
    coverage_end: null,
    verified_swaps: 1,
    first_discovery_source: 'EXPLORATION',
    last_investigated_at: null,
  }),
  wallet(9, {
    lifecycle: 'REJECTED',
    eligibility: 'EXCLUDED_BY_POLICY',
    operator_excluded: true,
    qualification: null,
    priority_score: '0.0000',
    history_stage: 'NONE',
    coverage_state: 'NOT_INVESTIGATED',
  }),
];

export function radarWallets(v: RadarVariant, lifecycle: string | null): RadarWalletsResponse {
  const items = v === 'empty' ? [] : WALLETS.filter((w) => !lifecycle || w.lifecycle === lifecycle);
  return { items, next_cursor: null, sort: 'PRIORITY' };
}

export function radarWallet(address: string): RadarWalletDetail | null {
  const w = WALLETS.find((x) => x.address === address);
  if (!w) return null;
  return {
    wallet: w,
    discovery_sources: [w.first_discovery_source, 'WALLET_HISTORY'],
    metrics: {
      policy: 'radar-priority-1',
      computed_at: at(1),
      priority: {
        score: w.priority_score ?? '0',
        activity: '0.8120',
        recency: '0.9500',
        evidence_quality: '0.9100',
        evidence_parts: ['0.95', '0.90', '0.88'],
        performance: '0.4410',
        performance_status: 'SHRUNK',
        diversity: '0.6000',
      },
      activity: [
        {
          window: '24h',
          swaps: 12,
          buys: 6,
          sells: 6,
          distinct_tokens: 3,
          trading_days: 1,
          undated: 0,
        },
        {
          window: '7d',
          swaps: 61,
          buys: 31,
          sells: 30,
          distinct_tokens: 9,
          trading_days: 7,
          undated: 0,
        },
        {
          window: '30d',
          swaps: 204,
          buys: 103,
          sells: 101,
          distinct_tokens: 24,
          trading_days: 26,
          undated: 0,
        },
        {
          window: '90d',
          swaps: 412,
          buys: 207,
          sells: 205,
          distinct_tokens: 38,
          trading_days: 71,
          undated: 0,
        },
        {
          window: 'all',
          swaps: 412,
          buys: 207,
          sells: 205,
          distinct_tokens: 38,
          trading_days: 71,
          undated: 0,
        },
      ],
      screening: {
        outcomes: 188,
        disposals: 205,
        disposals_matched: 196,
        matched_coverage_bps: 9561,
        open_positions: 4,
        unevaluated_cycles: 2,
        ambiguous_tokens: 0,
        unknown_cost_tokens: 1,
        by_denomination: [
          {
            denomination: 'SOL',
            outcomes: 171,
            wins: 102,
            cost: '214503200000',
            proceeds: '251122870400',
            return_ratio: '1.170717',
            incomplete_fees: 0,
          },
          {
            denomination: 'USDC',
            outcomes: 17,
            wins: 8,
            cost: '4210000000',
            proceeds: '3987120000',
            return_ratio: '0.947059',
            incomplete_fees: 1,
          },
        ],
      },
      coverage: {
        state: w.coverage_state,
        signatures_discovered: 498,
        transactions_retrieved: 496,
        transactions_unavailable: 2,
        transactions_verified: 412,
        transactions_unsupported: 84,
        stage_completed: w.history_stage === 'NONE' ? null : w.history_stage,
        swaps_analysed: 412,
        swap_limit: 2000,
      },
      evidence: { total: 412, valid: 411, valid_finalized: 409, distinct_tokens: 38 },
    },
    snapshot_policy: 'radar-priority-1',
    retrieval: {
      signatures_discovered: 498,
      transactions_retrieved: 496,
      transactions_pending: 0,
      transactions_unavailable: 2,
      credits_spent: 1_011,
      history_exhausted: false,
    },
    monitoring: [],
    recent_jobs: [
      {
        job_id: '0195f0a1-0000-7000-8000-0000000000c1',
        work_class: 'EXTENDED_HISTORY',
        subject_kind: 'WALLET',
        subject: w.address,
        origin: 'AUTONOMOUS',
        bucket: null,
        priority: 571,
        state: 'COMPLETED',
        defer_reason: null,
        result_code: 'REQUESTED_SCOPE_COMPLETE',
        signature_limit: 500,
        lookback_days: 90,
        estimated_credits: 505,
        credits_spent: 498,
        signatures_found: 498,
        signatures_reused: 200,
        attempts: 1,
        backfill_status: 'COMPLETED',
        next_run_at: at(300),
        created_at: at(300),
        started_at: at(299),
        completed_at: at(280),
      },
    ],
  };
}

export function radarHistory(address: string): RadarHistoryResponse {
  return {
    address,
    items: [
      {
        from_state: 'PRIORITIZED',
        to_state: 'MONITORED',
        trigger: 'MONITORING',
        reasons: ['SLOT_ASSIGNED'],
        policy: 'radar-lifecycle-1',
        transitioned_at: at(270),
      },
      {
        from_state: 'PRELIMINARY_QUALIFIED',
        to_state: 'PRIORITIZED',
        trigger: 'STAGE_C_COMPLETE',
        reasons: ['READY_FOR_DEEPER_ANALYSIS'],
        policy: 'radar-lifecycle-1',
        transitioned_at: at(280),
      },
      {
        from_state: null,
        to_state: 'DISCOVERED',
        trigger: 'SCREENING',
        reasons: ['VERIFIED_SWAP_IN_SAMPLED_POOL'],
        policy: 'radar-lifecycle-1',
        transitioned_at: at(900),
      },
    ],
  };
}

export function radarEvidence(address: string): RadarEvidenceResponse {
  const sig = (i: number) => `5ynthRadarSig${String(i).padStart(3, '1')}`.padEnd(88, 'K');
  return {
    address,
    items: [0, 1, 2].map((i) => ({
      swap_id: `swap:${sig(i)}:2`,
      signature: sig(i),
      slot: 330_000_000 - i * 41,
      block_time: at(30 + i * 11),
      token_mint: radarAddr('MemeMint'),
      quote_mint: 'So11111111111111111111111111111111111111112',
      pool: radarAddr('Poo1'),
      protocol: 'RAYDIUM_AMM_V4',
      economic_role: i % 2 === 0 ? 'BUYER' : 'SELLER',
      attribution: 'VERIFIED_OWNER',
      finality: i === 0 ? 'OBSERVED_CONFIRMED' : 'FINALIZED',
      discovery_source: 'ESTABLISHED',
      swap_revision: 2,
      valid: i !== 2,
      invalidation_reason: i === 2 ? 'RETRACTED_ORPHANED' : null,
      invalidated_at: i === 2 ? at(20) : null,
      discovered_at: at(29 + i * 11),
    })),
  };
}

export function radarMarkets(v: RadarVariant): RadarMarketsResponse {
  if (v === 'empty') return { items: [], next_cursor: null };
  const market = (
    i: number,
    symbol: string | null,
    over: Partial<RadarMarketsResponse['items'][number]>,
  ): RadarMarketsResponse['items'][number] => ({
    mint: radarAddr(`Mint${String(i)}`),
    symbol,
    name: null,
    classification: 'MEME_CANDIDATE',
    classification_reasons: ['LAUNCHPAD_TOKEN'],
    bucket: 'ESTABLISHED',
    sources: ['JUPITER_TOP_TRADED'],
    promotional_only: false,
    selection_state: 'SELECTED',
    selection_reasons: ['BUCKET_ALLOCATION'],
    activity_state: 'ACTIVE',
    market_priority: '0.734100',
    coverage_state: 'SAMPLED',
    samples_completed: 4,
    wallets_found: 61,
    first_discovered_at: at(1_400 + i * 60),
    last_observed_at: at(2),
    stale: false,
    last_sampled_at: at(6),
    pools: [
      {
        pool: radarAddr(`Poo1${String(i)}`),
        quote_mint: 'So11111111111111111111111111111111111111112',
        dex_id: 'raydium',
        validation: 'SUPPORTED',
        protocol: 'RAYDIUM_AMM_V4',
        liquidity_usd: '842310.55',
        volume_24h_usd: '5120044.10',
        txns_1h: 1_204,
        txns_24h: 21_880,
        observed_at: at(2),
        last_sampled_at: at(6),
        samples_completed: 4,
      },
    ],
    ...over,
  });
  return {
    items: [
      market(1, 'FIXA', {}),
      market(2, 'FIXB', { bucket: 'EMERGING', wallets_found: 18, market_priority: '0.512000' }),
      market(3, null, {
        bucket: 'EXPLORATION',
        selection_state: 'CANDIDATE',
        selection_reasons: ['NOT_ALLOCATED'],
        wallets_found: 0,
        samples_completed: 0,
        last_sampled_at: null,
        coverage_state: 'NOT_SAMPLED',
      }),
      market(4, 'FIXD', {
        classification: 'GENERAL_SPECULATIVE_TOKEN',
        sources: ['DEXSCREENER_BOOSTS'],
        promotional_only: true,
        selection_state: 'EXCLUDED',
        selection_reasons: ['PROMOTIONAL_ONLY'],
        stale: true,
        last_observed_at: at(95),
        pools: [],
        wallets_found: 0,
      }),
      market(5, 'FIXE', {
        selection_state: 'DEFERRED',
        selection_reasons: ['MISSING_LIQUIDITY'],
        pools: [
          {
            pool: radarAddr('Poo15'),
            quote_mint: null,
            dex_id: 'meteora',
            validation: 'UNSUPPORTED_PROGRAM',
            protocol: null,
            liquidity_usd: null,
            volume_24h_usd: null,
            txns_1h: null,
            txns_24h: null,
            observed_at: at(4),
            last_sampled_at: null,
            samples_completed: 0,
          },
        ],
        wallets_found: 0,
      }),
    ],
    next_cursor: null,
  };
}

export function radarMonitoring(v: RadarVariant): RadarMonitoringResponse {
  return {
    capacity: 10,
    live_available: false,
    items:
      v === 'empty'
        ? []
        : WALLETS.filter((w) => w.lifecycle === 'MONITORED').map((w) => ({
            address: w.address,
            mode: 'PERIODIC_HISTORY_REFRESH',
            data_engine_target: null,
            priority_at_start: '69.1200',
            started_at: at(270),
            ended_at: null,
            end_reason: null,
          })),
  };
}

export function radarJobs(v: RadarVariant): RadarJobsResponse {
  const status = radarStatus(v);
  if (v === 'empty') return { totals: status.jobs, items: [] };
  const job = (
    i: number,
    over: Partial<RadarJobsResponse['items'][number]>,
  ): RadarJobsResponse['items'][number] => ({
    job_id: `0195f0a1-0000-7000-8000-0000000001${String(i).padStart(2, '0')}`,
    work_class: 'INITIAL_HISTORY',
    subject_kind: 'WALLET',
    subject: radarAddr(`Wa11et${String(i)}`),
    origin: 'AUTONOMOUS',
    bucket: null,
    priority: 420,
    state: 'PENDING',
    defer_reason: null,
    result_code: null,
    signature_limit: 50,
    lookback_days: 7,
    estimated_credits: 51,
    credits_spent: 0,
    signatures_found: 0,
    signatures_reused: 0,
    attempts: 0,
    backfill_status: null,
    next_run_at: at(0),
    created_at: at(i * 3),
    started_at: null,
    completed_at: null,
    ...over,
  });
  return {
    totals: status.jobs,
    items: [
      job(1, {
        work_class: 'TRANSACTION_SAMPLING',
        subject_kind: 'POOL',
        subject: radarAddr('Poo11'),
        bucket: 'ESTABLISHED',
        state: 'WAITING_DATA',
        signature_limit: 25,
        estimated_credits: 26,
        credits_spent: 3,
        signatures_found: 25,
      }),
      job(2, { state: 'RUNNING', credits_spent: 4, signatures_found: 19 }),
      job(3, { state: 'DEFERRED', defer_reason: 'INVESTIGATION_CAPACITY' }),
      job(4, {
        work_class: 'EXPANDED_HISTORY',
        state: 'COMPLETED',
        result_code: 'REQUESTED_SCOPE_COMPLETE',
        signature_limit: 200,
        estimated_credits: 202,
        credits_spent: 141,
        signatures_found: 140,
      }),
      job(5, { state: 'FAILED', result_code: 'PROVIDER_HISTORY_UNAVAILABLE', credits_spent: 1 }),
      job(6, { origin: 'OPERATOR', state: 'CANCELLED', result_code: 'CANCELLED_BY_OPERATOR' }),
    ],
  };
}

export function radarCycles(v: RadarVariant): RadarCyclesResponse {
  if (v === 'empty') return { items: [] };
  return {
    items: [0, 1, 2, 3].map((i) => ({
      cycle_id: `0195f0a1-0000-7000-8000-0000000002${String(i).padStart(2, '0')}`,
      trigger: i === 3 ? 'OPERATOR' : 'SCHEDULED',
      status: i === 1 ? 'PARTIAL' : v === 'paused' && i === 0 ? 'DEFERRED' : 'COMPLETED',
      budget_state: 'NORMAL',
      deferred_reason: v === 'paused' && i === 0 ? 'OPERATOR_PAUSE' : null,
      observations: 120 - i * 7,
      markets_new: 3 - Math.min(i, 3),
      markets_selected: 5,
      samples_scheduled: i === 1 ? 3 : 5,
      credits_estimated: 130,
      started_at: at(2 + i * 5),
      finished_at: at(2 + i * 5),
    })),
  };
}

export function radarSources(v: RadarVariant): RadarSourcesResponse {
  if (v === 'empty') return { window_hours: 24, items: [] };
  const src = (
    source: string,
    over: Partial<RadarSourcesResponse['items'][number]>,
  ): RadarSourcesResponse['items'][number] => ({
    source,
    runs: 288,
    runs_ok: 288,
    runs_deferred: 0,
    runs_failed: 0,
    items: 14_400,
    markets_new: 22,
    provider_requests: 288,
    credits_estimated: 0,
    last_run_at: at(2),
    last_status: 'OK',
    evidence: 2_140,
    unique_wallets: 311,
    eligible_wallets: 102,
    investigated_wallets: 61,
    qualified_wallets: 4,
    ...over,
  });
  return {
    window_hours: 24,
    items: [
      src('DEXSCREENER_BOOSTS', {
        runs_ok: 0,
        runs_deferred: 288,
        items: 0,
        markets_new: 0,
        provider_requests: 0,
        last_status: 'DISABLED',
        evidence: 0,
        unique_wallets: 0,
        eligible_wallets: 0,
        investigated_wallets: 0,
        qualified_wallets: 0,
      }),
      src('JUPITER_RECENT', {
        runs_ok: 286,
        runs_failed: 2,
        last_status: 'PROVIDER_COOLDOWN',
        unique_wallets: 140,
        qualified_wallets: 1,
      }),
      src('JUPITER_TOP_TRADED', {}),
      src('LOCAL_ACTIVITY', {
        provider_requests: 0,
        items: 1_210,
        markets_new: 4,
        unique_wallets: 57,
        eligible_wallets: 20,
        investigated_wallets: 9,
        qualified_wallets: 1,
      }),
    ],
  };
}
