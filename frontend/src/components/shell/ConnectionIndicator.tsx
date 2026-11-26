import { useNow } from '@/hooks/useNow.ts';
import { formatLatencyUs } from '@/lib/format/number.ts';
import { formatDuration } from '@/lib/format/time.ts';

import { StatusIndicator } from '../ui/StatusIndicator.tsx';
import { Tooltip } from '../ui/Tooltip.tsx';
import { useConnection } from './useConnection.ts';

/** Backend + database connectivity, from the real readiness endpoint. */
export function ConnectionIndicator() {
  const { state, dbLatencyUs } = useConnection();
  const now = useNow(1_000);

  const view = (() => {
    switch (state.kind) {
      case 'connecting':
        return {
          tone: 'neutral' as const,
          label: 'Connecting',
          detail: undefined,
          tip: 'Waiting for the first readiness report.',
        };
      case 'connected':
        return {
          tone: 'positive' as const,
          label: 'Backend ready',
          detail: `DB ${formatLatencyUs(dbLatencyUs)}`,
          tip: 'Backend reachable, PostgreSQL available.',
        };
      case 'degraded':
        return {
          tone: 'warning' as const,
          label: 'Degraded',
          detail: 'DB unavailable',
          tip: state.reason,
        };
      case 'stale':
        return {
          tone: 'warning' as const,
          label: 'Stale',
          detail: formatDuration((now - state.lastSeenAt) / 1_000),
          tip: `${state.reason} Showing the last known state.`,
        };
      case 'offline':
        return {
          tone: 'negative' as const,
          label: 'Backend offline',
          detail: undefined,
          tip: state.reason,
        };
    }
  })();

  return (
    <Tooltip content={view.tip} side="bottom">
      <span
        tabIndex={0}
        data-testid="connection-indicator"
        data-state={state.kind}
        aria-label={`Backend connection: ${view.label}${view.detail ? `, ${view.detail}` : ''}`}
        className="rounded-xs"
      >
        <StatusIndicator tone={view.tone} label={view.label} detail={view.detail} />
      </span>
    </Tooltip>
  );
}
