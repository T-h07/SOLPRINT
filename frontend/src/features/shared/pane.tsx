/**
 * Shared pane plumbing for monitoring workspaces (Gateway, Data Engine):
 * backend-unreachable notice, data age and table skeleton rows. Visual treatment lives in `components/ui`.
 */
import { Button } from '@/components/ui/Button.tsx';
import { DataState, Skeleton } from '@/components/ui/DataState.tsx';
import { DataAge } from '@/components/ui/Metric.tsx';
import { Td, Tr } from '@/components/ui/Table.tsx';
import type { ApiFailure } from '@/lib/api/client.ts';
import { STALE_AFTER_MS } from '@/lib/api/queries.ts';

export function Unreachable({
  what,
  onRetry,
  detail = 'No values are shown until the backend answers. Retrying is safe: it reads local state only.',
}: {
  what: string;
  onRetry: () => void;
  detail?: string;
}) {
  return (
    <DataState
      kind="error"
      title={`${what} unavailable: backend unreachable.`}
      detail={detail}
      action={
        <Button size="sm" onClick={onRetry}>
          Retry
        </Button>
      }
    />
  );
}

/**
 * Failure notice that says *which* layer failed: the backend process, its
 * database, or the response contract. Retrying is always a local read.
 */
export function LoadFailure({
  what,
  failure,
  onRetry,
}: {
  what: string;
  failure: ApiFailure;
  onRetry: () => void;
}) {
  const title =
    failure.kind === 'http' && failure.status === 503
      ? `${what} unavailable: the backend could not read its database.`
      : failure.kind === 'timeout'
        ? `${what} unavailable: the backend did not respond in time.`
        : failure.kind === 'invalid'
          ? `${what} unavailable: the response did not match the expected contract.`
          : failure.kind === 'http'
            ? `${what} unavailable: request failed (${failure.status.toString()}).`
            : `${what} unavailable: backend unreachable.`;
  return (
    <DataState
      kind="error"
      title={title}
      detail="No values are shown until the data can be read. Retrying is safe: it reads local state only."
      action={
        <Button size="sm" onClick={onRetry}>
          Retry
        </Button>
      }
    />
  );
}

export function Age({ at, staleAfterMs = STALE_AFTER_MS }: { at: number; staleAfterMs?: number }) {
  return at > 0 ? <DataAge at={at} staleAfterMs={staleAfterMs} /> : null;
}

export function RowsSkeleton({ cols, rows = 4 }: { cols: number; rows?: number }) {
  return (
    <>
      {Array.from({ length: rows }, (_, i) => (
        <Tr key={i} aria-busy="true">
          <Td colSpan={cols}>
            <Skeleton className="h-3 w-full" />
          </Td>
        </Tr>
      ))}
    </>
  );
}
