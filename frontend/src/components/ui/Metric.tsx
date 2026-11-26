import type { ReactNode } from 'react';

import { useNow } from '@/hooks/useNow.ts';
import { cn } from '@/lib/cn.ts';
import { formatAge, formatDuration, formatUtc } from '@/lib/format/time.ts';

import { Skeleton } from './DataState.tsx';
import { Tooltip } from './Tooltip.tsx';

/**
 * Label + value cell. `loading` reserves the exact value height (no layout
 * shift); `unavailable` shows an em dash plus a reason instead of a fake 0.
 */
export function MetricCell({
  label,
  value,
  unit,
  delta,
  size = 'md',
  loading = false,
  unavailable,
  className,
}: {
  label: string;
  value?: ReactNode;
  unit?: string;
  delta?: ReactNode;
  size?: 'md' | 'lg';
  loading?: boolean;
  /** Reason the value is not available. Renders "—" with this text. */
  unavailable?: string | undefined;
  className?: string | undefined;
}) {
  const valueClass = size === 'lg' ? 'text-xl font-medium' : 'text-base font-medium';
  const skeletonClass = size === 'lg' ? 'h-8 w-28' : 'h-5 w-20';
  return (
    <div className={cn('flex min-w-0 flex-col gap-1', className)} aria-busy={loading || undefined}>
      <span className="text-2xs font-medium tracking-wider text-fg-muted uppercase">{label}</span>
      {loading ? (
        <Skeleton className={skeletonClass} />
      ) : unavailable !== undefined ? (
        <span className={cn('flex items-baseline gap-2', valueClass)}>
          <span className="text-fg-muted" aria-hidden="true">
            —
          </span>
          <span className="text-xs font-normal text-fg-muted">{unavailable}</span>
        </span>
      ) : (
        <span className={cn('num flex items-baseline gap-1.5 text-fg', valueClass)}>
          {value}
          {unit ? <span className="text-xs font-normal text-fg-muted">{unit}</span> : null}
          {delta ? <span className="text-xs font-normal">{delta}</span> : null}
        </span>
      )}
    </div>
  );
}

/**
 * Absolute UTC timestamp in a semantic <time> element; relative age is
 * offered as a tooltip.
 */
export function Timestamp({ iso, className }: { iso: string; className?: string }) {
  return (
    <time dateTime={iso} className={cn('num', className)}>
      {formatUtc(iso)}
    </time>
  );
}

/**
 * Data age primitive: "12s ago", or "STALE · 2m 14s" once older than
 * `staleAfterMs`. Absolute UTC time in the tooltip.
 */
export function DataAge({
  at,
  staleAfterMs,
  prefix = 'Updated',
}: {
  at: number;
  staleAfterMs: number;
  prefix?: string;
}) {
  const now = useNow(1_000);
  const ageMs = now - at;
  const stale = ageMs > staleAfterMs;
  return (
    <Tooltip content={formatUtc(at)}>
      <span
        tabIndex={0}
        className={cn('num text-xs whitespace-nowrap', stale ? 'text-warning' : 'text-fg-muted')}
      >
        {stale ? (
          <>
            <span aria-hidden="true">▲ </span>STALE · {formatDuration(ageMs / 1_000)}
          </>
        ) : (
          `${prefix} ${formatAge(at, now)}`
        )}
      </span>
    </Tooltip>
  );
}
