/**
 * GATEWAY monitoring panes. Every value comes from the backend's local
 * GATEWAY state or stored aggregates; polling these endpoints never sends a
 * request to Helius, Jupiter, DEX Screener or the public RPC.
 */
import type { UseQueryResult } from '@tanstack/react-query';
import type { ReactNode } from 'react';

import { Button } from '@/components/ui/Button.tsx';
import { DataState, Skeleton } from '@/components/ui/DataState.tsx';
import { DataAge, MetricCell, Timestamp } from '@/components/ui/Metric.tsx';
import { DefinitionList, Panel } from '@/components/ui/Panel.tsx';
import { StatusIndicator } from '@/components/ui/StatusIndicator.tsx';
import { TBody, THead, Table, Td, Th, Tr } from '@/components/ui/Table.tsx';
import { type ApiResponse, describeFailure } from '@/lib/api/client.ts';
import {
  STALE_AFTER_MS,
  USAGE_STALE_AFTER_MS,
  useGatewayBudgets,
  useGatewayCache,
  useGatewayMetrics,
  useGatewayProviders,
  useGatewayStatus,
  useGatewayStreams,
  useGatewayUsage,
} from '@/lib/api/queries.ts';
import { cn } from '@/lib/cn.ts';
import { formatBytes, formatInteger, formatLatencyUs, formatShare } from '@/lib/format/number.ts';
import { formatDuration } from '@/lib/format/time.ts';
import type { BudgetDto, GatewayProvider, ProviderId } from '@/types/api.ts';

import {
  admittedRange,
  budgetLabel,
  circuitLabel,
  continuityLabel,
  healthLabel,
  streamLabel,
} from './labels.ts';

const providerName: Record<ProviderId, string> = {
  HELIUS: 'Helius',
  JUPITER: 'Jupiter',
  DEX_SCREENER: 'DEX Screener',
  SOLANA_RPC: 'Solana RPC',
};

function useApi<T>(q: UseQueryResult<ApiResponse<T>>) {
  return {
    data: q.data?.data,
    loading: q.isPending && !q.error,
    error: q.error ? describeFailure(q.error) : null,
    updatedAt: q.dataUpdatedAt,
    retry: () => void q.refetch(),
  };
}

function Unreachable({ what, onRetry }: { what: string; onRetry: () => void }) {
  return (
    <DataState
      kind="error"
      title={`${what} unavailable: backend unreachable.`}
      detail="No values are shown until the backend answers. Retrying is safe: it reads local state only."
      action={
        <Button size="sm" onClick={onRetry}>
          Retry
        </Button>
      }
    />
  );
}

function Age({ at, staleAfterMs = STALE_AFTER_MS }: { at: number; staleAfterMs?: number }) {
  return at > 0 ? <DataAge at={at} staleAfterMs={staleAfterMs} /> : null;
}

function RowsSkeleton({ cols, rows = 4 }: { cols: number; rows?: number }) {
  return (
    <>
      {Array.from({ length: rows }, (_, i) => (
        <Tr key={i}>
          <Td colSpan={cols}>
            <Skeleton className="h-3 w-full" />
          </Td>
        </Tr>
      ))}
    </>
  );
}

const n = (v: number) => formatInteger(v);

// ---------------------------------------------------------------- budget

/** Credit budget: the one GATEWAY number that can stop work. */
export function BudgetPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useGatewayBudgets());
  const budget = data?.budgets[0];
  return (
    <Panel
      title="Helius credit budget"
      meta={data ? <Age at={updatedAt} /> : null}
      className={className}
    >
      {error && !data ? (
        <Unreachable what="Credit budget" onRetry={retry} />
      ) : data && !budget ? (
        <DataState kind="empty" title="No metered provider is configured." />
      ) : (
        <BudgetBody budget={budget} loading={loading} />
      )}
    </Panel>
  );
}

function BudgetBody({ budget: b, loading }: { budget: BudgetDto | undefined; loading: boolean }) {
  const state = b ? budgetLabel[b.state] : null;
  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4">
        <MetricCell
          label="Budget state"
          loading={loading}
          value={state ? <StatusIndicator tone={state.tone} label={state.label} /> : null}
        />
        <MetricCell
          size="lg"
          label={b ? `Credits used · of ${n(b.credit_limit)}` : 'Credits used'}
          loading={loading}
          value={b ? n(b.credits_used) : null}
        />
        <MetricCell label="Remaining" loading={loading} value={b ? n(b.credits_remaining) : null} />
        <MetricCell
          label="Admits"
          loading={loading}
          value={b ? admittedRange(b.admits_up_to) : null}
        />
      </div>
      {b ? <BudgetMeter budget={b} /> : <Skeleton className="h-10 w-full" />}
      {/* Sized like the details below, so data arriving does not push the page. */}
      {b ? null : <Skeleton className="h-96 w-full sm:h-56 xl:h-44" />}
      {b ? (
        <DefinitionList
          items={[
            {
              term: 'Cycle',
              wrap: true,
              value: (
                <span className="num">
                  <Timestamp iso={b.cycle_start} /> → <Timestamp iso={b.cycle_end} />
                </span>
              ),
            },
            {
              term: 'Projected at cycle end',
              wrap: true,
              value:
                b.projected_cycle_credits === null ? (
                  <span className="text-fg-muted">Not projected during the first hour</span>
                ) : (
                  <span className="num">
                    {n(b.projected_cycle_credits)}{' '}
                    <span className="text-fg-muted">
                      ({formatShare(b.projected_cycle_credits, b.credit_limit)} of limit, linear)
                    </span>
                  </span>
                ),
            },
            {
              term: 'Streams',
              wrap: true,
              value: (
                <span className="num">
                  {n(b.stream_credits)} credits · {formatBytes(b.stream_bytes)} received
                </span>
              ),
            },
            {
              term: 'Denied requests',
              wrap: true,
              value: (
                <span className="num text-fg-secondary">
                  {b.denied_by_priority
                    .map((d) => `${d.priority} ${formatInteger(d.count)}`)
                    .join(' · ')}
                </span>
              ),
            },
            {
              term: 'Ledger',
              wrap: true,
              value: b.ledger_loaded ? (
                <span className="text-fg-secondary">
                  Persisted
                  {b.last_persisted_at ? (
                    <>
                      {' '}
                      · last write <Timestamp iso={b.last_persisted_at} />
                    </>
                  ) : null}
                  {b.credits_pending_persist > 0 ? (
                    <span className="num"> · {n(b.credits_pending_persist)} pending</span>
                  ) : null}
                </span>
              ) : (
                <StatusIndicator
                  tone="warning"
                  label="Not loaded"
                  detail="cycle usage unknown; only P0–P2 admitted"
                />
              ),
            },
          ]}
        />
      ) : null}
      {b ? (
        <p className="m-0 border-t border-line-subtle pt-2 text-xs text-fg-muted">
          Estimated: SOLPRINT&apos;s own accounting of the requests it sent, at the documented
          per-method costs. Usage by other applications on the same key is not included;
          Helius&apos;s dashboard is authoritative for billing.
        </p>
      ) : null}
    </div>
  );
}

/**
 * Usage against thresholds. The bar is a visual aid only; every threshold
 * is also written out as text below it.
 */
function BudgetMeter({ budget: b }: { budget: BudgetDto }) {
  const limit = Math.max(b.credit_limit, 1);
  const pct = (v: number) => `${Math.min(100, (v / limit) * 100).toFixed(2)}%`;
  const over = b.credits_used >= b.conservation_at;
  const marks: { at: number; label: string }[] = [
    { at: b.conservation_at, label: 'Conservation' },
    { at: b.critical_at, label: 'Critical' },
    { at: b.normal_credits, label: 'Reserve' },
  ];
  return (
    <div className="flex flex-col gap-1.5">
      <div
        role="meter"
        aria-label="Helius credits used"
        aria-valuemin={0}
        aria-valuemax={b.credit_limit}
        aria-valuenow={Math.min(b.credits_used, b.credit_limit)}
        aria-valuetext={`${n(b.credits_used)} of ${n(b.credit_limit)} credits (${formatShare(b.credits_used, b.credit_limit)})`}
        className="relative h-2 w-full rounded-xs border border-line bg-raised"
      >
        <div
          className={cn('absolute inset-y-0 left-0', over ? 'bg-warning' : 'bg-fg-secondary')}
          style={{ width: pct(b.credits_used) }}
        />
        {marks.map((m) => (
          <span
            key={m.label}
            aria-hidden="true"
            className="absolute -inset-y-1 w-px bg-fg-muted"
            style={{ left: pct(m.at) }}
          />
        ))}
      </div>
      <p className="num m-0 flex flex-wrap gap-x-4 gap-y-0.5 text-2xs text-fg-muted">
        <span>Used {formatShare(b.credits_used, b.credit_limit)}</span>
        <span>Conservation ≥ {n(b.conservation_at)}</span>
        <span>Critical ≥ {n(b.critical_at)}</span>
        <span>
          Reserve {n(b.reserve_credits)} (≥ {n(b.normal_credits)})
        </span>
        <span>Hard limit {n(b.credit_limit)}</span>
      </p>
    </div>
  );
}

// ---------------------------------------------------------------- overview

export function GatewayStatePanel({ className }: { className?: string }) {
  const { data: s, loading, error, updatedAt, retry } = useApi(useGatewayStatus());
  return (
    <Panel title="Gateway" meta={s ? <Age at={updatedAt} /> : null} className={className}>
      {error && !s ? (
        <Unreachable what="Gateway state" onRetry={retry} />
      ) : loading || !s ? (
        <div className="flex flex-col gap-2" aria-busy="true">
          {[0, 1, 2, 3, 4].map((i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : (
        <DefinitionList
          items={[
            {
              term: 'Gateway',
              value: s.enabled ? (
                <StatusIndicator tone="positive" label="Enabled" />
              ) : (
                <StatusIndicator tone="neutral" label="Disabled" detail="GATEWAY_ENABLED=false" />
              ),
            },
            {
              term: 'Execution',
              value: <span className="text-fg-secondary">None · read-only access layer</span>,
            },
            {
              term: 'Streams',
              wrap: true,
              value: s.streams.state ? (
                <StatusIndicator
                  tone={streamLabel[s.streams.state].tone}
                  label={streamLabel[s.streams.state].label}
                  detail={`${n(s.streams.subscriptions)} subscriptions`}
                />
              ) : (
                <StatusIndicator
                  tone="neutral"
                  label="Off"
                  detail={s.streams.disabled_reason ?? undefined}
                />
              ),
            },
            {
              term: 'Queue',
              wrap: true,
              value: (
                <span className="num">
                  {n(s.scheduler.global_waiting)} waiting · limit {n(s.scheduler.global_limit)}
                </span>
              ),
            },
            {
              term: 'Cache',
              value: (
                <span className="num">
                  {n(s.cache.entries)} / {n(s.cache.max_entries)} entries
                </span>
              ),
            },
            {
              term: 'Persistence',
              wrap: true,
              value:
                s.persistence.last_flush_ok === false ? (
                  <StatusIndicator
                    tone="warning"
                    label="Write failing"
                    detail={`${n(s.persistence.flush_failures)} failures; usage kept in memory`}
                  />
                ) : s.persistence.last_flush_at ? (
                  <span className="text-fg-secondary">
                    Last flush <Timestamp iso={s.persistence.last_flush_at} />
                  </span>
                ) : (
                  <span className="text-fg-muted">No flush yet</span>
                ),
            },
            {
              term: 'Uptime',
              value: <span className="num">{formatDuration(s.uptime_seconds)}</span>,
            },
          ]}
        />
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- providers

export function ProviderHealthPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useGatewayProviders());
  return (
    <Panel
      title="Providers"
      flush
      meta={data ? <Age at={updatedAt} /> : null}
      className={className}
    >
      {error && !data ? (
        <div className="p-3">
          <Unreachable what="Provider health" onRetry={retry} />
        </div>
      ) : (
        <>
          <Table label="Provider health, limits and traffic">
            <THead>
              <Tr>
                <Th>Provider</Th>
                <Th>Health</Th>
                <Th className="hidden md:table-cell">Circuit</Th>
                <Th numeric className="hidden lg:table-cell">
                  In flight
                </Th>
                <Th numeric className="hidden lg:table-cell">
                  Queued
                </Th>
                <Th numeric className="hidden sm:table-cell">
                  Requests
                </Th>
                <Th numeric className="hidden sm:table-cell">
                  Errors
                </Th>
                <Th numeric className="hidden xl:table-cell">
                  429s
                </Th>
                <Th numeric className="hidden md:table-cell">
                  p50
                </Th>
                <Th numeric className="hidden md:table-cell">
                  p95
                </Th>
                <Th numeric className="hidden xl:table-cell">
                  p99
                </Th>
                <Th numeric className="hidden 2xl:table-cell">
                  Last success
                </Th>
              </Tr>
            </THead>
            <TBody>
              {loading || !data ? (
                <RowsSkeleton cols={12} />
              ) : (
                data.providers.map((p) => <ProviderRow key={p.id} p={p} />)
              )}
            </TBody>
          </Table>
          <p className="m-0 border-t border-line-subtle px-3 py-2 text-xs text-fg-muted">
            Health is derived from real requests only; there is no background probing. Latency
            figures are histogram bucket bounds since the backend started.
          </p>
        </>
      )}
    </Panel>
  );
}

function ProviderRow({ p }: { p: GatewayProvider }) {
  const h = healthLabel[p.health.state];
  const c = circuitLabel[p.circuit.state];
  const limited = p.limiters.reduce((sum, l) => sum + l.upstream_limited, 0);
  const lat = (v: number | null) => (v === null ? '—' : formatLatencyUs(v));
  return (
    <Tr>
      <Td>
        <span className="font-medium text-fg">{p.name}</span>
        {p.disabled_reason ? (
          <span className="block text-xs text-fg-muted">{p.disabled_reason}</span>
        ) : null}
      </Td>
      <Td>
        <StatusIndicator
          tone={h.tone}
          label={h.label}
          detail={
            p.health.recent_total > 0
              ? `${n(p.health.recent_failures)}/${n(p.health.recent_total)} failed`
              : undefined
          }
        />
      </Td>
      <Td className="hidden md:table-cell">
        <StatusIndicator
          tone={c.tone}
          label={c.label}
          detail={
            p.circuit.retry_in_ms !== null
              ? `retry ${formatDuration(p.circuit.retry_in_ms / 1_000)}`
              : undefined
          }
        />
      </Td>
      <Td numeric className="hidden lg:table-cell">
        {p.lane ? `${n(p.lane.in_flight)} / ${n(p.lane.max_in_flight)}` : '—'}
      </Td>
      <Td numeric className="hidden lg:table-cell">
        {p.lane ? n(p.lane.queued) : '—'}
      </Td>
      <Td numeric className="hidden sm:table-cell">
        {n(p.traffic.requests)}
      </Td>
      <Td numeric className="hidden sm:table-cell">
        {n(p.traffic.errors)}
      </Td>
      <Td numeric className="hidden xl:table-cell">
        {n(limited)}
      </Td>
      <Td numeric className="hidden md:table-cell">
        {lat(p.latency.p50_us)}
      </Td>
      <Td numeric className="hidden md:table-cell">
        {lat(p.latency.p95_us)}
      </Td>
      <Td numeric className="hidden xl:table-cell">
        {lat(p.latency.p99_us)}
      </Td>
      <Td numeric className="hidden text-fg-muted 2xl:table-cell">
        {p.health.last_success_at ? <Timestamp iso={p.health.last_success_at} /> : 'Never'}
      </Td>
    </Tr>
  );
}

// ---------------------------------------------------------------- operations

export function OperationsPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useGatewayMetrics());
  const ops = data?.operations ?? [];
  return (
    <Panel
      title="Operations"
      flush
      meta={data ? <Age at={updatedAt} /> : null}
      className={className}
    >
      {error && !data ? (
        <div className="p-3">
          <Unreachable what="Operation metrics" onRetry={retry} />
        </div>
      ) : data && ops.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title="No provider request has been made since the backend started."
            detail="GATEWAY only contacts providers when a module asks; monitoring does not."
          />
        </div>
      ) : (
        <Table label="Per-operation request metrics">
          <THead>
            <Tr>
              <Th>Operation</Th>
              <Th className="hidden sm:table-cell">Provider</Th>
              <Th numeric>OK</Th>
              <Th numeric>Errors</Th>
              <Th numeric className="hidden md:table-cell">
                Cache hits
              </Th>
              <Th numeric className="hidden xl:table-cell">
                Coalesced
              </Th>
              <Th numeric className="hidden xl:table-cell">
                Retries
              </Th>
              <Th numeric className="hidden lg:table-cell">
                p50
              </Th>
              <Th numeric className="hidden lg:table-cell">
                p95
              </Th>
              <Th numeric className="hidden 2xl:table-cell">
                p99
              </Th>
              <Th className="hidden lg:table-cell">Last error</Th>
            </Tr>
          </THead>
          <TBody>
            {loading || !data ? (
              <RowsSkeleton cols={11} />
            ) : (
              ops.map((o) => (
                <Tr key={`${o.provider}:${o.operation}`}>
                  <Td>
                    <span className="font-mono text-xs text-fg">{o.operation}</span>
                    <span className="block text-xs text-fg-muted sm:hidden">
                      {providerName[o.provider]}
                    </span>
                  </Td>
                  <Td className="hidden text-fg-secondary sm:table-cell">
                    {providerName[o.provider]}
                  </Td>
                  <Td numeric>{n(o.success)}</Td>
                  <Td numeric className={o.errors > 0 ? 'text-warning' : undefined}>
                    {n(o.errors)}
                  </Td>
                  <Td numeric className="hidden md:table-cell">
                    {n(o.cache_hits)}
                  </Td>
                  <Td numeric className="hidden xl:table-cell">
                    {n(o.coalesced)}
                  </Td>
                  <Td numeric className="hidden xl:table-cell">
                    {n(o.retries)}
                  </Td>
                  <Td numeric className="hidden lg:table-cell">
                    {formatLatencyUs(o.latency.p50_us)}
                  </Td>
                  <Td numeric className="hidden lg:table-cell">
                    {formatLatencyUs(o.latency.p95_us)}
                  </Td>
                  <Td numeric className="hidden 2xl:table-cell">
                    {formatLatencyUs(o.latency.p99_us)}
                  </Td>
                  <Td className="hidden text-xs text-fg-muted lg:table-cell">
                    {o.last_error_code ? (
                      <span>
                        <span className="font-mono">{o.last_error_code}</span>
                        {o.last_error_at ? (
                          <>
                            {' · '}
                            <Timestamp iso={o.last_error_at} />
                          </>
                        ) : null}
                      </span>
                    ) : (
                      '—'
                    )}
                  </Td>
                </Tr>
              ))
            )}
          </TBody>
        </Table>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- usage

export function UsagePanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useGatewayUsage());
  const totals = new Map<ProviderId, { requests: number; credits: number; bytes: number }>();
  for (const h of data?.history ?? []) {
    const t = totals.get(h.provider) ?? { requests: 0, credits: 0, bytes: 0 };
    t.requests += h.requests;
    t.credits += h.credits;
    t.bytes += h.bytes;
    totals.set(h.provider, t);
  }
  return (
    <Panel
      title="Usage · last 24 h"
      flush
      meta={data ? <Age at={updatedAt} staleAfterMs={USAGE_STALE_AFTER_MS} /> : null}
      className={className}
    >
      {error && !data ? (
        <div className="p-3">
          <Unreachable what="Usage" onRetry={retry} />
        </div>
      ) : (
        <>
          {data && !data.history_available ? (
            <div className="border-b border-line-subtle px-3 py-2">
              <DataState
                kind="partial"
                title="Stored history unavailable (database unreachable)."
                detail="Only totals since this process started are shown."
              />
            </div>
          ) : null}
          <Table label="Provider usage">
            <THead>
              <Tr>
                <Th>Provider</Th>
                <Th numeric>Requests 24 h</Th>
                <Th numeric>Credits 24 h</Th>
                <Th numeric className="hidden sm:table-cell">
                  Bytes 24 h
                </Th>
                <Th numeric className="hidden 2xl:table-cell">
                  Since start
                </Th>
              </Tr>
            </THead>
            <TBody>
              {loading || !data ? (
                <RowsSkeleton cols={5} />
              ) : (
                data.process.map((p) => {
                  const t = totals.get(p.provider);
                  const stored = data.history_available;
                  return (
                    <Tr key={p.provider}>
                      <Td className="font-medium text-fg">{providerName[p.provider]}</Td>
                      <Td numeric>{stored ? n(t?.requests ?? 0) : '—'}</Td>
                      <Td numeric>{stored ? n(t?.credits ?? 0) : '—'}</Td>
                      <Td numeric className="hidden sm:table-cell">
                        {stored ? formatBytes(t?.bytes ?? 0) : '—'}
                      </Td>
                      <Td numeric className="hidden text-fg-secondary 2xl:table-cell">
                        {n(p.requests)}
                      </Td>
                    </Tr>
                  );
                })
              )}
            </TBody>
          </Table>
          <p className="m-0 border-t border-line-subtle px-3 py-2 text-xs text-fg-muted">
            Stored totals lag by up to one flush interval. Only Helius is metered in credits.
          </p>
        </>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- streams

export function StreamsPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useGatewayStreams());
  const c = data?.connection;
  let body: ReactNode;
  if (error && !data) {
    body = <Unreachable what="Stream state" onRetry={retry} />;
  } else if (loading || !data) {
    body = <Skeleton className="h-24 w-full" />;
  } else if (!data.enabled || !c) {
    body = (
      <DataState
        kind="empty"
        title={`Streams are off${data.disabled_reason ? ` (${data.disabled_reason})` : ''}.`}
        detail="No WebSocket connection is open, so no stream credits are spent."
      />
    );
  } else {
    const s = streamLabel[c.state];
    body = (
      <div className="flex flex-col gap-3">
        <DefinitionList
          items={[
            {
              term: 'Connection',
              value: (
                <StatusIndicator
                  tone={s.tone}
                  label={s.label}
                  detail={`generation ${n(c.generation)}`}
                />
              ),
            },
            {
              term: 'Last message',
              value: c.last_message_at ? (
                <Timestamp iso={c.last_message_at} />
              ) : (
                <span className="text-fg-muted">None yet</span>
              ),
            },
            {
              term: 'Received',
              wrap: true,
              value: (
                <span className="num">
                  {formatBytes(c.bytes_received)} · {n(c.credits_charged)} credits
                </span>
              ),
            },
            {
              term: 'Events',
              wrap: true,
              value: (
                <span className="num">
                  {n(c.events_emitted)} delivered
                  {c.events_dropped > 0 ? (
                    <span className="text-warning"> · {n(c.events_dropped)} dropped</span>
                  ) : null}
                </span>
              ),
            },
            {
              term: 'Continuity',
              wrap: true,
              value: (
                <span className="num">
                  {n(c.reconnects)} reconnects · {n(c.gaps)} gaps · {n(c.recovered)} recovered ·{' '}
                  {n(c.duplicates_suppressed)} duplicates
                </span>
              ),
            },
            ...(c.last_error
              ? [
                  {
                    term: 'Last error',
                    value: <span className="text-fg-secondary">{c.last_error}</span>,
                  },
                ]
              : []),
          ]}
        />
        {data.subscriptions.length > 0 ? (
          <Table label="Stream subscriptions">
            <THead>
              <Tr>
                <Th>Kind</Th>
                <Th>Continuity</Th>
                <Th numeric>Events</Th>
                <Th numeric className="hidden sm:table-cell">
                  Last slot
                </Th>
              </Tr>
            </THead>
            <TBody>
              {data.subscriptions.map((sub) => {
                const cl = continuityLabel[sub.continuity];
                return (
                  <Tr key={sub.id}>
                    <Td>
                      <span className="font-mono text-xs">{sub.kind}</span>
                      {sub.durable_key ? (
                        <span className="block text-xs text-fg-muted">{sub.durable_key}</span>
                      ) : null}
                    </Td>
                    <Td>
                      <StatusIndicator tone={cl.tone} label={cl.label} />
                    </Td>
                    <Td numeric>{n(sub.notifications)}</Td>
                    <Td numeric className="hidden sm:table-cell">
                      {sub.last_slot === null ? '—' : n(sub.last_slot)}
                    </Td>
                  </Tr>
                );
              })}
            </TBody>
          </Table>
        ) : (
          <DataState kind="empty" title="Connected without subscriptions." />
        )}
      </div>
    );
  }
  return (
    <Panel title="Streams" meta={data ? <Age at={updatedAt} /> : null} className={className}>
      {body}
    </Panel>
  );
}

// ---------------------------------------------------------------- cache

export function CachePanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useGatewayCache());
  return (
    <Panel
      title="Response cache"
      flush
      meta={data ? <Age at={updatedAt} /> : null}
      className={className}
    >
      {error && !data ? (
        <div className="p-3">
          <Unreachable what="Cache statistics" onRetry={retry} />
        </div>
      ) : data && data.namespaces.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title={`Cache is empty (${n(data.max_entries)} entries allowed).`}
            detail="Entries appear after the first cacheable provider response."
          />
        </div>
      ) : (
        <Table label="Cache statistics by operation">
          <THead>
            <Tr>
              <Th>Namespace</Th>
              <Th numeric>Hits</Th>
              <Th numeric>Misses</Th>
              <Th numeric className="hidden sm:table-cell">
                Hit rate
              </Th>
              <Th numeric className="hidden md:table-cell">
                Stale
              </Th>
            </Tr>
          </THead>
          <TBody>
            {loading || !data ? (
              <RowsSkeleton cols={5} rows={3} />
            ) : (
              data.namespaces.map((ns) => (
                <Tr key={ns.namespace}>
                  <Td className="font-mono text-xs">{ns.namespace}</Td>
                  <Td numeric>{n(ns.hits)}</Td>
                  <Td numeric>{n(ns.misses)}</Td>
                  <Td numeric className="hidden sm:table-cell">
                    {formatShare(ns.hits, ns.hits + ns.misses)}
                  </Td>
                  <Td numeric className="hidden md:table-cell">
                    {n(ns.stale_hits)}
                  </Td>
                </Tr>
              ))
            )}
          </TBody>
        </Table>
      )}
    </Panel>
  );
}
