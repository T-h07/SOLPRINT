/**
 * Derives the single backend connection state shown in the shell from the
 * readiness query. Pure function so every state is unit-testable.
 */
import type { ReadyResponse } from '@/types/api.ts';

import { describeFailure } from './client.ts';
import { STALE_AFTER_MS } from './queries.ts';

export type ConnectionState =
  /** First request in flight; nothing known yet. */
  | { kind: 'connecting' }
  /** Backend and database answered ready. */
  | { kind: 'connected'; checkedAt: number }
  /** Backend answered, a critical dependency (database) is down. */
  | { kind: 'degraded'; checkedAt: number; reason: string }
  /** Backend unreachable and no recent good data. */
  | { kind: 'offline'; reason: string; lastSeenAt: number | null }
  /** Last good answer is older than the staleness threshold. */
  | { kind: 'stale'; lastSeenAt: number; reason: string };

export interface ReadinessQueryLike {
  data: { status: number; data: ReadyResponse } | undefined;
  dataUpdatedAt: number;
  error: unknown;
  isPending: boolean;
}

export function deriveConnection(q: ReadinessQueryLike, now: number): ConnectionState {
  if (q.isPending && !q.error) return { kind: 'connecting' };

  const lastSeenAt = q.data ? q.dataUpdatedAt : null;

  if (q.error) {
    const failure = describeFailure(q.error);
    if (lastSeenAt !== null && now - lastSeenAt <= STALE_AFTER_MS) {
      // A single failed poll after recent success: keep the last state but
      // never pretend it is newer than it is (the age is shown alongside).
      return fromData(q.data?.data, lastSeenAt);
    }
    if (lastSeenAt !== null) {
      return { kind: 'stale', lastSeenAt, reason: failure.message };
    }
    return { kind: 'offline', reason: failure.message, lastSeenAt: null };
  }

  if (lastSeenAt !== null && now - lastSeenAt > STALE_AFTER_MS) {
    return { kind: 'stale', lastSeenAt, reason: 'No fresh readiness report.' };
  }
  return fromData(q.data?.data, lastSeenAt ?? now);
}

function fromData(data: ReadyResponse | undefined, at: number): ConnectionState {
  if (!data) return { kind: 'connecting' };
  if (data.status === 'READY') return { kind: 'connected', checkedAt: at };
  return {
    kind: 'degraded',
    checkedAt: at,
    reason:
      data.checks.database.status === 'UNAVAILABLE'
        ? 'Database unavailable.'
        : 'A critical dependency is unavailable.',
  };
}
