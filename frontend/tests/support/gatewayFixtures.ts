/**
 * Synthetic GATEWAY responses for UI and visual tests only. Values are dense
 * enough to exercise layout, but they are not measurements and never appear
 * in the application.
 */
import type {
  GatewayBudgetsResponse,
  GatewayCacheResponse,
  GatewayMetricsResponse,
  GatewayProvider,
  GatewayProvidersResponse,
  GatewayStatus,
  GatewayStreamsResponse,
  GatewayUsageResponse,
  ProviderId,
} from '../../src/types/api.ts';

const AT = '2026-11-26T12:00:00.000Z';

export type GatewayVariant = 'active' | 'idle' | 'pressure';

function provider(
  id: ProviderId,
  name: string,
  over: Partial<GatewayProvider> = {},
): GatewayProvider {
  return {
    id,
    name,
    enabled: true,
    disabled_reason: null,
    health: {
      state: 'UNKNOWN',
      reason: null,
      last_success_at: null,
      last_failure_at: null,
      last_error_code: null,
      recent_failures: 0,
      recent_total: 0,
    },
    circuit: { state: 'CLOSED', consecutive_failures: 0, retry_in_ms: null, times_opened: 0 },
    lane: {
      max_in_flight: 4,
      queue_limit: 128,
      in_flight: 0,
      queued: 0,
      peak_in_flight: 0,
      admitted: 0,
      queued_total: 0,
      rejected: 0,
      timed_out: 0,
    },
    limiters: [],
    traffic: {
      requests: 0,
      credits: 0,
      bytes: 0,
      success: 0,
      errors: 0,
      cache_hits: 0,
      coalesced: 0,
      retries: 0,
      fallbacks: 0,
    },
    latency: { p50_us: null, p95_us: null, p99_us: null },
    ...over,
  };
}

function healthy(requests: number, errors: number, p50: number, p95: number, p99: number) {
  return {
    health: {
      state: 'HEALTHY' as const,
      reason: null,
      last_success_at: '2026-11-26T11:59:58.412Z',
      last_failure_at: errors > 0 ? '2026-11-26T11:41:07.000Z' : null,
      last_error_code: errors > 0 ? 'TIMEOUT' : null,
      recent_failures: Math.min(errors, 2),
      recent_total: 64,
    },
    traffic: {
      requests,
      credits: 0,
      bytes: requests * 1_830,
      success: requests - errors,
      errors,
      cache_hits: Math.floor(requests * 0.6),
      coalesced: Math.floor(requests * 0.04),
      retries: errors,
      fallbacks: 0,
    },
    latency: { p50_us: p50, p95_us: p95, p99_us: p99 },
  };
}

export function gatewayProviders(v: GatewayVariant): GatewayProvidersResponse {
  if (v === 'idle') {
    return {
      generated_at: AT,
      providers: [
        provider('HELIUS', 'Helius'),
        provider('JUPITER', 'Jupiter', { lane: null }),
        provider('DEX_SCREENER', 'DEX Screener'),
        provider('SOLANA_RPC', 'Solana RPC', {
          enabled: false,
          disabled_reason: 'SOLANA_FALLBACK_ENABLED=false',
          health: { ...provider('SOLANA_RPC', '').health, state: 'DISABLED' },
        }),
      ],
    };
  }
  const helius = provider('HELIUS', 'Helius', {
    ...healthy(18_432, 7, 50_000, 250_000, 500_000),
    lane: {
      max_in_flight: 4,
      queue_limit: 128,
      in_flight: 2,
      queued: 0,
      peak_in_flight: 4,
      admitted: 18_432,
      queued_total: 311,
      rejected: 0,
      timed_out: 0,
    },
  });
  helius.traffic.credits = 18_425;
  const jupiter =
    v === 'pressure'
      ? provider('JUPITER', 'Jupiter', {
          health: {
            state: 'UNAVAILABLE',
            reason: 'circuit open',
            last_success_at: '2026-11-26T11:52:10.000Z',
            last_failure_at: '2026-11-26T11:59:40.000Z',
            last_error_code: 'HTTP_5XX',
            recent_failures: 9,
            recent_total: 20,
          },
          circuit: { state: 'OPEN', consecutive_failures: 5, retry_in_ms: 42_000, times_opened: 2 },
          traffic: { ...healthy(2_104, 41, 0, 0, 0).traffic },
          latency: { p50_us: 100_000, p95_us: 1_000_000, p99_us: 2_500_000 },
        })
      : provider('JUPITER', 'Jupiter', healthy(2_104, 0, 100_000, 250_000, 500_000));
  return {
    generated_at: AT,
    providers: [
      helius,
      jupiter,
      provider('DEX_SCREENER', 'DEX Screener', healthy(611, 1, 100_000, 500_000, 1_000_000)),
      provider('SOLANA_RPC', 'Solana RPC'),
    ],
  };
}

export function gatewayBudgets(v: GatewayVariant): GatewayBudgetsResponse {
  const used = v === 'idle' ? 0 : v === 'pressure' ? 614_220 : 212_904;
  return {
    generated_at: AT,
    budgets: [
      {
        provider: 'HELIUS',
        state: v === 'pressure' ? 'CONSERVATION' : 'NORMAL',
        admits_up_to: v === 'pressure' ? 'P3' : 'P5',
        cycle_start: '2026-11-01T00:00:00.000Z',
        cycle_end: '2026-12-01T00:00:00.000Z',
        credit_limit: 1_000_000,
        reserve_credits: 200_000,
        normal_credits: 800_000,
        conservation_at: 560_000,
        critical_at: 720_000,
        credits_used: used,
        credits_pending_persist: v === 'idle' ? 0 : 37,
        credits_remaining: 1_000_000 - used,
        projected_cycle_credits: v === 'idle' ? null : Math.round((used * 30) / 25.5),
        stream_bytes: v === 'idle' ? 0 : 48_211_904,
        stream_credits: v === 'idle' ? 0 : 965,
        denied_by_priority: (['P0', 'P1', 'P2', 'P3', 'P4', 'P5'] as const).map((priority) => ({
          priority,
          count: v === 'pressure' && (priority === 'P4' || priority === 'P5') ? 312 : 0,
        })),
        ledger_loaded: true,
        last_persisted_at: v === 'idle' ? null : '2026-11-26T11:59:41.000Z',
        persist_failures: 0,
      },
    ],
  };
}

export function gatewayStatus(v: GatewayVariant): GatewayStatus {
  const [b] = gatewayBudgets(v).budgets;
  if (!b) throw new Error('fixture has no budget');
  const p = gatewayProviders(v).providers;
  return {
    enabled: true,
    execution_capability: 'NONE',
    generated_at: AT,
    started_at: '2026-11-26T09:14:05.000Z',
    uptime_seconds: 9_955,
    providers: p.map((x) => ({
      id: x.id,
      name: x.name,
      enabled: x.enabled,
      health: x.health.state,
      circuit: x.circuit.state,
    })),
    budget: {
      provider: 'HELIUS',
      state: b.state,
      credits_used: b.credits_used,
      credit_limit: b.credit_limit,
      credits_remaining: b.credits_remaining,
    },
    streams:
      v === 'idle'
        ? {
            enabled: false,
            disabled_reason: 'GATEWAY_STREAMS_ENABLED=false',
            state: null,
            subscriptions: 0,
          }
        : { enabled: true, disabled_reason: null, state: 'CONNECTED', subscriptions: 3 },
    persistence: {
      ledger_loaded: true,
      last_flush_at: v === 'idle' ? null : '2026-11-26T11:59:41.000Z',
      last_flush_ok: v === 'idle' ? null : true,
      flush_failures: 0,
      last_prune_at: null,
    },
    scheduler: { global_limit: 512, global_waiting: v === 'pressure' ? 17 : 0 },
    cache: { entries: v === 'idle' ? 0 : 1_284, max_entries: 10_000 },
  };
}

export function gatewayMetrics(v: GatewayVariant): GatewayMetricsResponse {
  if (v === 'idle') return { generated_at: AT, operations: [] };
  const op = (
    provider: ProviderId,
    operation: string,
    success: number,
    errors: number,
    p50: number,
    p95: number,
    p99: number,
  ) => ({
    provider,
    operation,
    success,
    errors,
    cache_hits: Math.floor(success * 0.5),
    coalesced: Math.floor(success * 0.03),
    retries: errors,
    fallbacks: 0,
    latency: { p50_us: p50, p95_us: p95, p99_us: p99 },
    errors_by_code: errors > 0 ? [{ code: 'TIMEOUT', count: errors }] : [],
    last_error_code: errors > 0 ? 'TIMEOUT' : null,
    last_error_at: errors > 0 ? '2026-11-26T11:41:07.000Z' : null,
  });
  return {
    generated_at: AT,
    operations: [
      op('HELIUS', 'getSlot', 9_120, 0, 25_000, 100_000, 250_000),
      op('HELIUS', 'getAccountInfo', 4_388, 3, 50_000, 250_000, 500_000),
      op('HELIUS', 'getSignaturesForAddress', 2_911, 4, 100_000, 500_000, 1_000_000),
      op('HELIUS', 'getTransaction', 2_006, 0, 100_000, 250_000, 500_000),
      op('JUPITER', 'jupiter.price', 1_880, v === 'pressure' ? 41 : 0, 100_000, 250_000, 500_000),
      op('JUPITER', 'jupiter.quote', 224, 0, 250_000, 500_000, 1_000_000),
      op('DEX_SCREENER', 'dex.tokens', 611, 1, 100_000, 500_000, 1_000_000),
    ],
  };
}

export function gatewayUsage(v: GatewayVariant, historyAvailable = true): GatewayUsageResponse {
  const hours = Array.from({ length: v === 'idle' ? 0 : 24 }, (_, i) =>
    new Date(Date.parse('2026-11-25T12:00:00.000Z') + i * 3_600_000).toISOString(),
  );
  return {
    generated_at: AT,
    since: '2026-11-25T12:00:00.000Z',
    process: (['HELIUS', 'JUPITER', 'DEX_SCREENER', 'SOLANA_RPC'] as const).map((provider) => ({
      provider,
      requests:
        v === 'idle' ? 0 : provider === 'HELIUS' ? 18_432 : provider === 'SOLANA_RPC' ? 0 : 600,
      credits: v === 'idle' || provider !== 'HELIUS' ? 0 : 18_425,
      bytes: v === 'idle' ? 0 : 1_204_000,
    })),
    history: historyAvailable
      ? hours.flatMap((hour_start) => [
          {
            provider: 'HELIUS' as const,
            hour_start,
            requests: 3_102,
            credits: 3_140,
            bytes: 5_812_000,
          },
          { provider: 'JUPITER' as const, hour_start, requests: 410, credits: 0, bytes: 402_000 },
        ])
      : [],
    history_available: historyAvailable,
  };
}

export function gatewayStreams(v: GatewayVariant): GatewayStreamsResponse {
  if (v === 'idle') {
    return {
      generated_at: AT,
      enabled: false,
      disabled_reason: 'GATEWAY_STREAMS_ENABLED=false',
      connection: null,
      subscriptions: [],
      checkpoints: [],
      checkpoints_available: true,
    };
  }
  return {
    generated_at: AT,
    enabled: true,
    disabled_reason: null,
    connection: {
      state: v === 'pressure' ? 'DEGRADED' : 'CONNECTED',
      generation: 3,
      connected_since: '2026-11-26T10:02:11.000Z',
      last_message_at: '2026-11-26T11:59:59.600Z',
      reconnects: 2,
      bytes_received: 48_211_904,
      credits_charged: 965,
      events_emitted: 412_009,
      events_dropped: v === 'pressure' ? 118 : 0,
      gaps: v === 'pressure' ? 1 : 0,
      recovered: 37,
      duplicates_suppressed: 12,
      last_error: 'heartbeat timeout',
      max_subscriptions: 100,
    },
    subscriptions: [
      {
        id: 1,
        kind: 'slot',
        commitment: null,
        durable_key: null,
        active: true,
        notifications: 386_120,
        dropped: 0,
        continuity: 'UNKNOWN',
        last_slot: 384_920_117,
      },
      {
        id: 2,
        kind: 'logs',
        commitment: 'confirmed',
        durable_key: 'test.logs.program',
        active: true,
        notifications: 25_870,
        dropped: 0,
        continuity: v === 'pressure' ? 'GAP' : 'RECOVERED',
        last_slot: 384_920_115,
      },
      {
        id: 3,
        kind: 'account',
        commitment: 'confirmed',
        durable_key: null,
        active: true,
        notifications: 19,
        dropped: 0,
        continuity: 'CONTINUOUS',
        last_slot: 384_919_904,
      },
    ],
    checkpoints: [],
    checkpoints_available: true,
  };
}

export function gatewayCache(v: GatewayVariant): GatewayCacheResponse {
  if (v === 'idle') return { generated_at: AT, entries: 0, max_entries: 10_000, namespaces: [] };
  const ns = (namespace: string, hits: number, misses: number) => ({
    namespace,
    hits,
    stale_hits: Math.floor(hits / 200),
    negative_hits: 0,
    misses,
    inserts: misses,
  });
  return {
    generated_at: AT,
    entries: 1_284,
    max_entries: 10_000,
    namespaces: [
      ns('getSlot', 6_204, 2_916),
      ns('getAccountInfo', 2_190, 2_198),
      ns('jupiter.price', 1_120, 760),
      ns('dex.tokens', 302, 309),
    ],
  };
}
