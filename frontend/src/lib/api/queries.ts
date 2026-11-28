/**
 * Server-state hooks (TanStack Query). Polling intervals are deliberately
 * modest: every poll is a local request, but nothing in the shell should
 * generate more traffic than it needs.
 */
import { useQuery } from '@tanstack/react-query';

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
  GatewayBudgetsResponse,
  GatewayCacheResponse,
  GatewayMetricsResponse,
  GatewayProvidersResponse,
  GatewayStatus,
  GatewayStreamsResponse,
  GatewayUsageResponse,
  RadarCyclesResponse,
  RadarEvidenceResponse,
  RadarHistoryResponse,
  RadarJobsResponse,
  RadarMarketsResponse,
  RadarMonitoringResponse,
  RadarSourcesResponse,
  RadarStatus,
  RadarWalletDetail,
  RadarWalletsResponse,
  ReadyResponse,
  SystemStatus,
  VersionInfo,
} from '@/types/api.ts';

import { type ApiResponse, getJson } from './client.ts';
import {
  isDataBackfills,
  isDataDecoders,
  isDataEvents,
  isDataIssues,
  isDataStatus,
  isDataSwaps,
  isDataTargets,
  isDataTransactionDetail,
  isDataTransactions,
  isGatewayBudgets,
  isGatewayCache,
  isGatewayMetrics,
  isGatewayProviders,
  isGatewayStatus,
  isGatewayStreams,
  isGatewayUsage,
  isRadarCycles,
  isRadarEvidence,
  isRadarHistory,
  isRadarJobs,
  isRadarMarkets,
  isRadarMonitoring,
  isRadarSources,
  isRadarStatus,
  isRadarWalletDetail,
  isRadarWallets,
  isReadyResponse,
  isSystemStatus,
  isVersionInfo,
} from './guards.ts';

export const POLL_INTERVAL_MS = 5_000;
/** Data older than this (three missed polls) is presented as STALE. */
export const STALE_AFTER_MS = 15_000;

export const queryKeys = {
  status: ['system', 'status'] as const,
  readiness: ['health', 'ready'] as const,
  version: ['system', 'version'] as const,
  gateway: (part: string) => ['gateway', part] as const,
  data: (part: string, ...args: (string | null)[]) => ['data', part, ...args] as const,
  radar: (part: string, ...args: (string | null)[]) => ['radar', part, ...args] as const,
};

/**
 * Stored usage changes on the backend flush interval (default 30 s), so it is
 * polled less often than live state.
 */
export const USAGE_POLL_INTERVAL_MS = 60_000;
export const USAGE_STALE_AFTER_MS = 180_000;

export function useSystemStatus() {
  return useQuery<ApiResponse<SystemStatus>>({
    queryKey: queryKeys.status,
    queryFn: ({ signal }) => getJson('/api/v1/system/status', { signal, guard: isSystemStatus }),
    refetchInterval: POLL_INTERVAL_MS,
  });
}

export function useReadiness() {
  return useQuery<ApiResponse<ReadyResponse>>({
    queryKey: queryKeys.readiness,
    queryFn: ({ signal }) =>
      getJson('/health/ready', { signal, guard: isReadyResponse, acceptStatuses: [503] }),
    refetchInterval: POLL_INTERVAL_MS,
  });
}

export function useVersion() {
  return useQuery<ApiResponse<VersionInfo>>({
    queryKey: queryKeys.version,
    queryFn: ({ signal }) => getJson('/api/v1/system/version', { signal, guard: isVersionInfo }),
    staleTime: Number.POSITIVE_INFINITY,
  });
}

// GATEWAY monitoring. Every endpoint reads local state or stored aggregates
// on the backend: polling them never causes a provider request.

function gatewayQuery<T>(
  part: string,
  path: string,
  guard: (v: unknown) => v is T,
  interval = POLL_INTERVAL_MS,
) {
  return {
    queryKey: queryKeys.gateway(part),
    queryFn: ({ signal }: { signal: AbortSignal }) => getJson(path, { signal, guard }),
    refetchInterval: interval,
  };
}

export function useGatewayStatus() {
  return useQuery<ApiResponse<GatewayStatus>>(
    gatewayQuery('status', '/api/v1/gateway/status', isGatewayStatus),
  );
}

export function useGatewayProviders() {
  return useQuery<ApiResponse<GatewayProvidersResponse>>(
    gatewayQuery('providers', '/api/v1/gateway/providers', isGatewayProviders),
  );
}

export function useGatewayBudgets() {
  return useQuery<ApiResponse<GatewayBudgetsResponse>>(
    gatewayQuery('budgets', '/api/v1/gateway/budgets', isGatewayBudgets),
  );
}

export function useGatewayMetrics() {
  return useQuery<ApiResponse<GatewayMetricsResponse>>(
    gatewayQuery('metrics', '/api/v1/gateway/metrics', isGatewayMetrics),
  );
}

export function useGatewayStreams() {
  return useQuery<ApiResponse<GatewayStreamsResponse>>(
    gatewayQuery('streams', '/api/v1/gateway/streams', isGatewayStreams),
  );
}

export function useGatewayCache() {
  return useQuery<ApiResponse<GatewayCacheResponse>>(
    gatewayQuery('cache', '/api/v1/gateway/cache', isGatewayCache),
  );
}

export function useGatewayUsage() {
  return useQuery<ApiResponse<GatewayUsageResponse>>(
    gatewayQuery('usage', '/api/v1/gateway/usage?hours=24', isGatewayUsage, USAGE_POLL_INTERVAL_MS),
  );
}

// DATA ENGINE. Endpoints read PostgreSQL and in-process counters only: polling
// them never causes a provider request and never enqueues work.

/** Decoded records change only as jobs complete, so lists poll less often. */
export const DATA_LIST_POLL_INTERVAL_MS = 15_000;
export const DATA_LIST_STALE_AFTER_MS = 45_000;

export function useDataStatus() {
  return useQuery<ApiResponse<DataEngineStatus>>({
    queryKey: queryKeys.data('status'),
    queryFn: ({ signal }) => getJson('/api/v1/data/status', { signal, guard: isDataStatus }),
    refetchInterval: POLL_INTERVAL_MS,
  });
}

function dataList<T>(
  part: string,
  path: string,
  guard: (v: unknown) => v is T,
  key: string | null = null,
) {
  return {
    queryKey: queryKeys.data(part, key),
    queryFn: ({ signal }: { signal: AbortSignal }) => getJson(path, { signal, guard }),
    refetchInterval: DATA_LIST_POLL_INTERVAL_MS,
  };
}

const PROTOCOL = /^[A-Z0-9_]{1,32}$/;

export function useDataSwaps(protocol: string | null) {
  const q = protocol && PROTOCOL.test(protocol) ? `&protocol=${protocol}` : '';
  return useQuery<ApiResponse<DataSwapsResponse>>(
    dataList('swaps', `/api/v1/data/swaps?limit=50${q}`, isDataSwaps, protocol),
  );
}

export function useDataTransactions() {
  return useQuery<ApiResponse<DataTransactionsResponse>>(
    dataList('transactions', '/api/v1/data/transactions?limit=25', isDataTransactions),
  );
}

export function useDataIssues() {
  return useQuery<ApiResponse<DataIssuesResponse>>(
    dataList('issues', '/api/v1/data/issues?limit=25', isDataIssues),
  );
}

export function useDataTargets() {
  return useQuery<ApiResponse<DataTargetsResponse>>(
    dataList('targets', '/api/v1/data/targets', isDataTargets),
  );
}

export function useDataBackfills() {
  return useQuery<ApiResponse<DataBackfillsResponse>>(
    dataList('backfills', '/api/v1/data/backfills?limit=10', isDataBackfills),
  );
}

export function useDataEvents() {
  return useQuery<ApiResponse<DataEventsResponse>>(
    dataList('events', '/api/v1/data/events?limit=20', isDataEvents),
  );
}

/** The decoder registry is fixed for the life of the backend process. */
export function useDataDecoders() {
  return useQuery<ApiResponse<DataDecodersResponse>>({
    queryKey: queryKeys.data('decoders'),
    queryFn: ({ signal }) => getJson('/api/v1/data/decoders', { signal, guard: isDataDecoders }),
    staleTime: Infinity,
  });
}

const SIGNATURE = /^[1-9A-HJ-NP-Za-km-z]{64,88}$/;

/** Detail for the inspector; disabled until a valid signature is selected. */
export function useDataTransaction(signature: string | null) {
  const valid = signature !== null && SIGNATURE.test(signature);
  return useQuery<ApiResponse<DataTransactionDetail>>({
    queryKey: queryKeys.data('transaction', signature),
    queryFn: ({ signal }) =>
      getJson(`/api/v1/data/transactions/${signature ?? ''}`, {
        signal,
        guard: isDataTransactionDetail,
      }),
    enabled: valid,
    refetchInterval: DATA_LIST_POLL_INTERVAL_MS,
  });
}

// RADAR. Endpoints read PostgreSQL, GATEWAY's in-memory budget snapshot and
// in-process counters only: polling them never causes a provider request and
// never schedules discovery or investigation work.

export const RADAR_LIST_POLL_INTERVAL_MS = 15_000;
export const RADAR_LIST_STALE_AFTER_MS = 45_000;

export function useRadarStatus() {
  return useQuery<ApiResponse<RadarStatus>>({
    queryKey: queryKeys.radar('status'),
    queryFn: ({ signal }) => getJson('/api/v1/radar/status', { signal, guard: isRadarStatus }),
    refetchInterval: POLL_INTERVAL_MS,
  });
}

function radarList<T>(
  part: string,
  path: string,
  guard: (v: unknown) => v is T,
  key: string | null = null,
  enabled = true,
) {
  return {
    queryKey: queryKeys.radar(part, key),
    queryFn: ({ signal }: { signal: AbortSignal }) => getJson(path, { signal, guard }),
    refetchInterval: RADAR_LIST_POLL_INTERVAL_MS,
    enabled,
  };
}

const ENUM = /^[A-Z0-9_]{1,40}$/;
const ADDRESS = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;

export type WalletSortKey = 'PRIORITY' | 'DISCOVERED' | 'LAST_TRADE';

export function useRadarWallets(lifecycle: string | null, sort: WalletSortKey) {
  const l = lifecycle && ENUM.test(lifecycle) ? `&lifecycle=${lifecycle}` : '';
  return useQuery<ApiResponse<RadarWalletsResponse>>(
    radarList(
      'wallets',
      `/api/v1/radar/wallets?limit=50&sort=${sort}${l}`,
      isRadarWallets,
      `${lifecycle ?? ''}:${sort}`,
    ),
  );
}

const validAddress = (a: string | null): a is string => a !== null && ADDRESS.test(a);

export function useRadarWallet(address: string | null) {
  return useQuery<ApiResponse<RadarWalletDetail>>(
    radarList(
      'wallet',
      `/api/v1/radar/wallets/${address ?? ''}`,
      isRadarWalletDetail,
      address,
      validAddress(address),
    ),
  );
}

export function useRadarWalletEvidence(address: string | null) {
  return useQuery<ApiResponse<RadarEvidenceResponse>>(
    radarList(
      'evidence',
      `/api/v1/radar/wallets/${address ?? ''}/evidence?limit=25`,
      isRadarEvidence,
      address,
      validAddress(address),
    ),
  );
}

export function useRadarWalletHistory(address: string | null) {
  return useQuery<ApiResponse<RadarHistoryResponse>>(
    radarList(
      'history',
      `/api/v1/radar/wallets/${address ?? ''}/history?limit=25`,
      isRadarHistory,
      address,
      validAddress(address),
    ),
  );
}

export function useRadarMarkets() {
  return useQuery<ApiResponse<RadarMarketsResponse>>(
    radarList('markets', '/api/v1/radar/markets?limit=25', isRadarMarkets),
  );
}

export function useRadarSources() {
  return useQuery<ApiResponse<RadarSourcesResponse>>(
    radarList('sources', '/api/v1/radar/sources?window_hours=24', isRadarSources),
  );
}

export function useRadarJobs() {
  return useQuery<ApiResponse<RadarJobsResponse>>(
    radarList('jobs', '/api/v1/radar/jobs?limit=20', isRadarJobs),
  );
}

export function useRadarMonitoring() {
  return useQuery<ApiResponse<RadarMonitoringResponse>>(
    radarList('monitoring', '/api/v1/radar/monitoring', isRadarMonitoring),
  );
}

export function useRadarCycles() {
  return useQuery<ApiResponse<RadarCyclesResponse>>(
    radarList('cycles', '/api/v1/radar/cycles?limit=8', isRadarCycles),
  );
}
