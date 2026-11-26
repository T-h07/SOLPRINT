/**
 * Server-state hooks (TanStack Query). Polling intervals are deliberately
 * modest: every poll is a local request, but nothing in the shell should
 * generate more traffic than it needs.
 */
import { useQuery } from '@tanstack/react-query';

import type { ReadyResponse, SystemStatus, VersionInfo } from '@/types/api.ts';

import { type ApiResponse, getJson } from './client.ts';
import { isReadyResponse, isSystemStatus, isVersionInfo } from './guards.ts';

export const POLL_INTERVAL_MS = 5_000;
/** Data older than this (three missed polls) is presented as STALE. */
export const STALE_AFTER_MS = 15_000;

export const queryKeys = {
  status: ['system', 'status'] as const,
  readiness: ['health', 'ready'] as const,
  version: ['system', 'version'] as const,
};

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
