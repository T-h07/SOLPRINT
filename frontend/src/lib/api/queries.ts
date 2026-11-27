/**
 * Server-state hooks (TanStack Query). Polling intervals are deliberately
 * modest: every poll is a local request, but nothing in the shell should
 * generate more traffic than it needs.
 */
import { useQuery } from '@tanstack/react-query';

import type {
  GatewayBudgetsResponse,
  GatewayCacheResponse,
  GatewayMetricsResponse,
  GatewayProvidersResponse,
  GatewayStatus,
  GatewayStreamsResponse,
  GatewayUsageResponse,
  ReadyResponse,
  SystemStatus,
  VersionInfo,
} from '@/types/api.ts';

import { type ApiResponse, getJson } from './client.ts';
import {
  isGatewayBudgets,
  isGatewayCache,
  isGatewayMetrics,
  isGatewayProviders,
  isGatewayStatus,
  isGatewayStreams,
  isGatewayUsage,
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
