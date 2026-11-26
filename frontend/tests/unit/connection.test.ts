import { describe, expect, it } from 'vitest';

import { ApiRequestError } from '@/lib/api/client.ts';
import { deriveConnection, type ReadinessQueryLike } from '@/lib/api/connection.ts';
import { STALE_AFTER_MS } from '@/lib/api/queries.ts';
import type { ReadyResponse } from '@/types/api.ts';

const NOW = 1_800_000_000_000;
const ready: ReadyResponse = {
  status: 'READY',
  checks: {
    database: { status: 'AVAILABLE', latency_us: 300, checked_at: '2026-11-26T12:00:00Z' },
  },
};
const notReady: ReadyResponse = {
  status: 'NOT_READY',
  checks: {
    database: { status: 'UNAVAILABLE', latency_us: null, checked_at: '2026-11-26T12:00:00Z' },
  },
};
const unreachable = new ApiRequestError({ kind: 'unreachable', message: 'Backend unreachable.' });

const q = (p: Partial<ReadinessQueryLike>): ReadinessQueryLike => ({
  data: undefined,
  dataUpdatedAt: 0,
  error: null,
  isPending: false,
  ...p,
});

describe('deriveConnection', () => {
  it('is connecting before the first answer', () => {
    expect(deriveConnection(q({ isPending: true }), NOW).kind).toBe('connecting');
  });

  it('is connected when ready', () => {
    const s = deriveConnection(q({ data: { status: 200, data: ready }, dataUpdatedAt: NOW }), NOW);
    expect(s).toEqual({ kind: 'connected', checkedAt: NOW });
  });

  it('is degraded when the database is down', () => {
    const s = deriveConnection(
      q({ data: { status: 503, data: notReady }, dataUpdatedAt: NOW }),
      NOW,
    );
    expect(s).toMatchObject({ kind: 'degraded', reason: 'Database unavailable.' });
  });

  it('is offline when the backend never answered', () => {
    const s = deriveConnection(q({ error: unreachable }), NOW);
    expect(s).toEqual({ kind: 'offline', reason: 'Backend unreachable.', lastSeenAt: null });
  });

  it('keeps the last state through one recent failed poll', () => {
    const at = NOW - 5_000;
    const s = deriveConnection(
      q({ data: { status: 200, data: ready }, dataUpdatedAt: at, error: unreachable }),
      NOW,
    );
    expect(s).toEqual({ kind: 'connected', checkedAt: at });
  });

  it('becomes stale once the last good answer is too old', () => {
    const at = NOW - STALE_AFTER_MS - 1;
    const failing = deriveConnection(
      q({ data: { status: 200, data: ready }, dataUpdatedAt: at, error: unreachable }),
      NOW,
    );
    expect(failing).toMatchObject({ kind: 'stale', lastSeenAt: at });
    const silent = deriveConnection(
      q({ data: { status: 200, data: ready }, dataUpdatedAt: at }),
      NOW,
    );
    expect(silent.kind).toBe('stale');
  });
});
