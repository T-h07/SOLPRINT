import { useNow } from '@/hooks/useNow.ts';
import { type ConnectionState, deriveConnection } from '@/lib/api/connection.ts';
import { useReadiness } from '@/lib/api/queries.ts';

/** Backend connection state derived from the real readiness endpoint. */
export function useConnection(): { state: ConnectionState; dbLatencyUs: number | null } {
  const readiness = useReadiness();
  const now = useNow(1_000);
  const state = deriveConnection(readiness, now);
  return { state, dbLatencyUs: readiness.data?.data.checks.database.latency_us ?? null };
}
