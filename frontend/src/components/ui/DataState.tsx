import type { ReactNode } from 'react';

import { cn } from '@/lib/cn.ts';

import { StatusIndicator, type Tone } from './StatusIndicator.tsx';

/**
 * Compact, operational notices for every non-"available" data state. One
 * line of fact plus optional detail and action; no illustrations, no
 * cheerful copy.
 *
 *   empty            "No qualified wallets yet. Radar last scanned 38s ago."
 *   error            what failed, where, and whether retry is safe
 *   stale            last-updated age with explicit STALE label
 *   partial          which sources are missing
 *   not-initialized  the producer does not exist / has never reported
 */
export type DataStateKind = 'empty' | 'error' | 'stale' | 'partial' | 'not-initialized';

const meta: Record<DataStateKind, { tone: Tone; label: string }> = {
  empty: { tone: 'neutral', label: 'Empty' },
  error: { tone: 'negative', label: 'Error' },
  stale: { tone: 'warning', label: 'Stale' },
  partial: { tone: 'warning', label: 'Partial' },
  'not-initialized': { tone: 'neutral', label: 'Not initialized' },
};

export function DataState({
  kind,
  title,
  detail,
  action,
  className,
}: {
  kind: DataStateKind;
  title: string;
  detail?: ReactNode;
  action?: ReactNode;
  className?: string | undefined;
}) {
  const m = meta[kind];
  return (
    <div
      role={kind === 'error' ? 'alert' : 'status'}
      className={cn('flex flex-wrap items-start gap-x-3 gap-y-1 py-1 text-sm', className)}
    >
      <StatusIndicator tone={m.tone} label={m.label} className="pt-0.5" />
      <div className="min-w-0 flex-1">
        <p className="m-0 text-fg">{title}</p>
        {detail ? <p className="m-0 text-xs text-fg-muted">{detail}</p> : null}
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  );
}

/** Geometry-matched placeholder. Reserve the real content's size. */
export function Skeleton({ className }: { className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn('block animate-skeleton rounded-xs bg-hover', className)}
    />
  );
}

/**
 * Wraps anything rendered from the fixture layer. The label is part of the
 * frame, so fixture data can never be mistaken for live data.
 */
export function FixtureFrame({
  children,
  label = 'Fixture · demo data',
  className,
}: {
  children: ReactNode;
  label?: string;
  className?: string | undefined;
}) {
  return (
    <div
      data-fixture="true"
      className={cn(
        'relative rounded-md border border-dashed border-warning/60 p-3 pt-5',
        className,
      )}
    >
      <span className="absolute -top-2.5 left-2 bg-canvas px-1.5 text-2xs font-semibold tracking-widest text-warning uppercase">
        {label}
      </span>
      {children}
    </div>
  );
}
