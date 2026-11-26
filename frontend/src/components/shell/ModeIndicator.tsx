import { useSystemStatus } from '@/lib/api/queries.ts';
import { cn } from '@/lib/cn.ts';

import { Skeleton } from '../ui/DataState.tsx';
import { StatusIndicator } from '../ui/StatusIndicator.tsx';

/**
 * Persistent operating-mode indicator. The backend is the only authority:
 * nothing here is derived from browser state, and if the backend cannot be
 * reached the mode is shown as UNVERIFIED rather than assumed.
 *
 * Text carries the meaning; colour and fill reinforce it:
 *   PAPER MODE      neutral bordered chip (safe default)
 *   LIVE TRADING    solid red chip (never reachable in Phase 00)
 *   MODE UNVERIFIED amber bordered chip with warning glyph
 */
export function ModeIndicator({ compact = false }: { compact?: boolean }) {
  const status = useSystemStatus();
  const trading = status.data?.data.trading;
  const lastKnown = status.error !== null && trading !== undefined;

  if (status.isPending) {
    return (
      <div className="flex items-center gap-3" aria-busy="true" aria-label="Loading operating mode">
        <Skeleton className="h-6 w-24" />
        {compact ? null : <Skeleton className="h-4 w-36" />}
      </div>
    );
  }

  if (!trading) {
    return (
      <div className="flex items-center gap-3" data-testid="mode-indicator" data-mode="unverified">
        <span className="inline-flex h-6 items-center gap-1.5 rounded-sm border border-warning px-2 text-xs font-semibold tracking-wider text-warning uppercase">
          <span aria-hidden="true">▲</span>Mode unverified
        </span>
        {compact ? null : (
          <span className="text-xs text-fg-muted">
            Backend unreachable · no execution in this build
          </span>
        )}
      </div>
    );
  }

  const live = trading.mode === 'LIVE';
  return (
    <div
      className="flex items-center gap-3"
      data-testid="mode-indicator"
      data-mode={live ? 'live' : 'paper'}
    >
      <span
        className={cn(
          'inline-flex h-6 items-center rounded-sm px-2 text-xs font-semibold tracking-wider uppercase',
          live ? 'bg-live text-live-fg' : 'border border-paper text-paper',
        )}
      >
        {live ? 'Live trading' : 'Paper mode'}
      </span>
      {compact ? null : trading.live_trading_enabled ? (
        <StatusIndicator tone="negative" label="Live trading enabled" />
      ) : (
        <StatusIndicator tone="neutral" label="Live trading disabled" />
      )}
      {lastKnown ? <span className="text-2xs text-warning uppercase">Last known</span> : null}
    </div>
  );
}
