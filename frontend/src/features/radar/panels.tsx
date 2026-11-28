/**
 * RADAR workspace panes. Every value comes from the backend's PostgreSQL
 * registry, GATEWAY's in-memory budget snapshot or RADAR's in-process
 * counters; polling these endpoints never sends a provider request and
 * never schedules work. Operator controls are the `solprint radar` CLI.
 *
 * A priority score orders RADAR's own investigation work. It is not a
 * prediction, a recommendation or a measure of skill, and the UI never
 * colours it as good or bad.
 */
import type { ReactNode } from 'react';

import { DataState, Skeleton } from '@/components/ui/DataState.tsx';
import { Select } from '@/components/ui/Field.tsx';
import { MetricCell, Timestamp } from '@/components/ui/Metric.tsx';
import { DefinitionList, Panel } from '@/components/ui/Panel.tsx';
import { StatusIndicator } from '@/components/ui/StatusIndicator.tsx';
import { TBody, THead, Table, Td, Th, Tr } from '@/components/ui/Table.tsx';
import { Id } from '@/features/data/panels.tsx';
import {
  RADAR_LIST_STALE_AFTER_MS,
  useRadarCycles,
  useRadarJobs,
  useRadarMarkets,
  useRadarMonitoring,
  useRadarSources,
  useRadarStatus,
  useRadarWallets,
  type WalletSortKey,
} from '@/lib/api/queries.ts';
import { formatDecimalString, formatInteger, formatShare } from '@/lib/format/number.ts';
import type { LabelCount, RadarMarketDto, RadarStatus, RadarWalletDto } from '@/types/api.ts';

import { Age, LoadFailure, RowsSkeleton } from '../shared/pane.tsx';
import { useApi } from '../shared/useApi.ts';
import {
  budgetModeLabel,
  cycleLabel,
  eligibilityLabel,
  formatScore,
  LIFECYCLE_ORDER,
  lifecycleLabel,
  monitoringLabel,
  poolLabel,
  qualificationLabel,
  radarCoverageLabel,
  radarJobLabel,
  radarName,
  selectionLabel,
  sourceRunLabel,
  workName,
} from './labels.ts';

const n = (v: number) => formatInteger(v);

const count = (list: LabelCount[] | undefined, label: string) =>
  list?.find((c) => c.label === label)?.count ?? 0;

const muted = (text: string) => <span className="text-fg-muted">{text}</span>;

function Breakdown({ items, label }: { items: { key: string; node: ReactNode }[]; label: string }) {
  if (items.length === 0) return <span className="text-xs text-fg-muted">None recorded</span>;
  return (
    <ul aria-label={label} className="m-0 flex list-none flex-wrap gap-x-4 gap-y-1 p-0 text-xs">
      {items.map((i) => (
        <li key={i.key} className="num whitespace-nowrap">
          {i.node}
        </li>
      ))}
    </ul>
  );
}

function counts(list: LabelCount[], name: (v: string) => string, label: string) {
  return (
    <Breakdown
      label={label}
      items={list
        .filter((c) => c.count > 0)
        .map((c) => ({
          key: c.label,
          node: (
            <>
              <span className="text-fg-muted">{name(c.label)}</span> {n(c.count)}
            </>
          ),
        }))}
    />
  );
}

/** "Off · …" / "On" / "Paused · reason": the one line that says whether RADAR spends credits. */
function DiscoveryState({ s }: { s: RadarStatus }) {
  if (!s.enabled) {
    return <StatusIndicator tone="neutral" label="Disabled" detail="RADAR_ENABLED=false" />;
  }
  if (s.paused) {
    return (
      <StatusIndicator
        tone="warning"
        label="Paused by operator"
        className="whitespace-normal"
        detail={s.pause_reason ? radarName(s.pause_reason) : undefined}
      />
    );
  }
  if (!s.auto_discovery_enabled) {
    return (
      <StatusIndicator
        tone="neutral"
        label="Autonomous discovery off"
        className="whitespace-normal"
        detail="operator-requested cycles only"
      />
    );
  }
  return (
    <StatusIndicator
      tone="info"
      label="Autonomous discovery on"
      detail="scheduled cycles"
      className="whitespace-normal"
    />
  );
}

// ---------------------------------------------------------------- discovery state

export function DiscoveryPanel({ className }: { className?: string }) {
  const { data: s, loading, error, updatedAt, retry } = useApi(useRadarStatus());
  const monitored = count(s?.registry.by_lifecycle, 'MONITORED');
  const investigating =
    count(s?.registry.by_lifecycle, 'HISTORY_QUEUED') +
    count(s?.registry.by_lifecycle, 'INVESTIGATING');
  const cap = s?.settings.max_live_monitored_wallets ?? 0;
  return (
    <Panel title="Discovery" meta={s ? <Age at={updatedAt} /> : null} className={className}>
      {error && !s ? (
        <LoadFailure what="RADAR state" failure={error} onRetry={retry} />
      ) : (
        <div className="flex flex-col gap-4">
          <div className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4">
            <MetricCell
              size="lg"
              label="Wallets · new 24 h"
              loading={loading}
              value={s ? `${n(s.registry.wallets)} · ${n(s.registry.wallets_new_24h)}` : null}
            />
            <MetricCell
              label="Investigating"
              loading={loading}
              value={s ? n(investigating) : null}
            />
            <MetricCell
              label={s ? `Monitored · of ${n(cap)}` : 'Monitored'}
              loading={loading}
              value={s ? n(monitored) : null}
            />
            <MetricCell
              label="Markets · selected"
              loading={loading}
              value={
                s
                  ? `${n(s.markets.markets)} · ${n(count(s.markets.by_selection, 'SELECTED'))}`
                  : null
              }
            />
          </div>
          {loading || !s ? (
            <Skeleton className="h-28 w-full" />
          ) : (
            <DefinitionList
              items={[
                { term: 'Discovery', wrap: true, value: <DiscoveryState s={s} /> },
                {
                  term: 'Process',
                  wrap: true,
                  value: s.running ? (
                    <StatusIndicator
                      tone="positive"
                      label="Running"
                      detail={
                        s.data_engine_running
                          ? 'data engine running'
                          : 'data engine not running · provider work deferred'
                      }
                    />
                  ) : (
                    <StatusIndicator
                      tone="neutral"
                      label="Not running"
                      detail="stored registry only"
                    />
                  ),
                },
                {
                  term: 'Execution',
                  value: <span className="text-fg-secondary">None · read-only discovery</span>,
                },
                {
                  term: 'Lifecycle',
                  wrap: true,
                  value: (
                    <Breakdown
                      label="Wallets by lifecycle"
                      items={LIFECYCLE_ORDER.filter(
                        (k) => count(s.registry.by_lifecycle, k) > 0,
                      ).map((k) => ({
                        key: k,
                        node: (
                          <>
                            <span className="text-fg-muted">{lifecycleLabel(k).label}</span>{' '}
                            {n(count(s.registry.by_lifecycle, k))}
                          </>
                        ),
                      }))}
                    />
                  ),
                },
                {
                  term: 'Evidence',
                  wrap: true,
                  value: (
                    <span className="num text-fg-secondary">
                      {n(s.registry.evidence)} verified swaps · {n(s.registry.evidence_invalidated)}{' '}
                      invalidated · {n(s.registry.reactivations)} reactivations
                    </span>
                  ),
                },
                {
                  term: 'Last cycle',
                  wrap: true,
                  value: s.last_cycle ? (
                    <span className="inline-flex flex-wrap items-center gap-x-2">
                      <StatusIndicator
                        tone={cycleLabel(s.last_cycle.status).tone}
                        label={cycleLabel(s.last_cycle.status).label}
                        detail={
                          s.last_cycle.deferred_reason
                            ? radarName(s.last_cycle.deferred_reason)
                            : undefined
                        }
                      />
                      <Timestamp iso={s.last_cycle.started_at} className="text-fg-secondary" />
                      <span className="num text-xs text-fg-muted">{n(s.cycles_24h)} in 24 h</span>
                    </span>
                  ) : (
                    muted('No discovery cycle has run.')
                  ),
                },
                {
                  term: 'Work queue',
                  wrap: true,
                  value:
                    s.jobs.open > 0 ? (
                      <span className="flex flex-col gap-1">
                        <span className="num text-fg-secondary">{n(s.jobs.open)} open</span>
                        {s.jobs.deferred_by_reason.length > 0
                          ? counts(s.jobs.deferred_by_reason, radarName, 'Deferred jobs by reason')
                          : null}
                      </span>
                    ) : (
                      muted('Nothing queued')
                    ),
                },
                {
                  term: 'Live monitoring',
                  wrap: true,
                  value: s.live_monitoring_available ? (
                    <span className="text-fg-secondary">Available · stream subscriptions</span>
                  ) : (
                    <span className="text-fg-muted">
                      Unavailable · monitored wallets use periodic history refresh
                    </span>
                  ),
                },
              ]}
            />
          )}
        </div>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- budget

export function BudgetPanel({ className }: { className?: string }) {
  const { data: s, loading, error, updatedAt, retry } = useApi(useRadarStatus());
  const b = s?.budget ?? null;
  const mode = b ? budgetModeLabel(b.mode) : null;
  return (
    <Panel title="Provider budget" meta={s ? <Age at={updatedAt} /> : null} className={className}>
      {error && !s ? (
        <LoadFailure what="RADAR budget" failure={error} onRetry={retry} />
      ) : loading || !s || !b || !mode ? (
        <div className="flex flex-col gap-2" aria-busy="true">
          {[0, 1, 2, 3, 4, 5].map((i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : (
        <DefinitionList
          items={[
            {
              term: 'Admission',
              wrap: true,
              value: (
                <StatusIndicator
                  tone={mode.tone}
                  label={mode.label}
                  detail={b.reason ? radarName(b.reason) : undefined}
                />
              ),
            },
            {
              term: 'Gateway budget',
              value: (
                <span className="text-fg-secondary">
                  {radarName(b.gateway_state)}
                  {b.ledger_loaded ? '' : ' · ledger not loaded'}
                </span>
              ),
            },
            {
              term: 'RADAR share',
              value: (
                <span className="num text-fg-secondary">
                  {formatShare(b.share_bps, 10_000, 0)} of remaining normal credits
                </span>
              ),
            },
            {
              term: 'Allowance',
              wrap: true,
              value: (
                <span className="num">
                  {n(b.available)} available{' '}
                  <span className="text-fg-muted">
                    of {n(b.allowance)} · {n(b.outstanding_estimates)} reserved by running jobs
                  </span>
                </span>
              ),
            },
            {
              term: 'Spent · 24 h',
              wrap: true,
              value: (
                <span className="flex flex-col gap-1">
                  <span className="num">{n(b.spent_24h)} credits</span>
                  {b.spent_24h_by_class.length > 0
                    ? counts(b.spent_24h_by_class, workName, 'Credits by work class')
                    : null}
                </span>
              ),
            },
            {
              term: 'History stages',
              wrap: true,
              value: (
                <span className="num text-fg-secondary">
                  {s.settings.history_stages.map((h, i) => (
                    <span key={h.stage} className="whitespace-nowrap">
                      {i > 0 ? ' · ' : ''}
                      {`${h.stage} ${n(h.days)} d / ${n(h.signatures)} sig`}
                    </span>
                  ))}
                </span>
              ),
            },
          ]}
        />
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- wallets

function WalletRow({ w, onOpen }: { w: RadarWalletDto; onOpen: (a: string) => void }) {
  const l = lifecycleLabel(w.lifecycle);
  const q = w.qualification ? qualificationLabel(w.qualification) : null;
  const c = radarCoverageLabel(w.coverage_state);
  const e = eligibilityLabel(w.eligibility);
  const m = monitoringLabel(w.monitoring_mode);
  return (
    <Tr>
      <Td>
        <button
          type="button"
          onClick={() => {
            onOpen(w.address);
          }}
          className="rounded-xs text-left text-fg underline decoration-line-control underline-offset-2 hover:decoration-fg"
          title={w.address}
        >
          <span className="sr-only">Inspect wallet </span>
          <Id value={w.address} />
        </button>
        <span className="block text-xs text-fg-muted">
          {e.label}
          {w.operator_excluded ? ' · operator excluded' : ''}
        </span>
      </Td>
      <Td numeric>{formatScore(w.priority_score)}</Td>
      <Td className="whitespace-normal">
        <StatusIndicator tone={l.tone} label={l.label} className="whitespace-normal" />
        {q ? <span className="block text-xs text-fg-secondary lg:hidden">{q.label}</span> : null}
      </Td>
      <Td className="hidden whitespace-normal lg:table-cell">
        {q ? <span className="text-fg-secondary">{q.label}</span> : muted('Not evaluated')}
      </Td>
      <Td className="hidden whitespace-normal md:table-cell">
        <span className="num text-fg-secondary">
          {w.history_stage === 'NONE' ? 'No history' : `Stage ${w.history_stage}`}
        </span>
        <span className="block">
          <StatusIndicator tone={c.tone} label={c.label} className="whitespace-normal" />
        </span>
      </Td>
      <Td numeric className="hidden sm:table-cell">
        {n(w.verified_swaps)}
        <span className="block text-fg-muted">
          {n(w.distinct_tokens)} {w.distinct_tokens === 1 ? 'token' : 'tokens'}
        </span>
      </Td>
      <Td className="hidden xl:table-cell">
        {w.last_verified_trade_at ? <Timestamp iso={w.last_verified_trade_at} /> : muted('—')}
      </Td>
      <Td className="hidden 2xl:table-cell">
        <StatusIndicator tone={m.tone} label={m.label} />
      </Td>
    </Tr>
  );
}

const SORTS: { key: WalletSortKey; label: string }[] = [
  { key: 'PRIORITY', label: 'Priority' },
  { key: 'LAST_TRADE', label: 'Last trade' },
  { key: 'DISCOVERED', label: 'Discovered' },
];

export function WalletsPanel({
  className,
  lifecycle,
  onLifecycle,
  sort,
  onSort,
  onOpen,
}: {
  className?: string;
  lifecycle: string | null;
  onLifecycle: (v: string | null) => void;
  sort: WalletSortKey;
  onSort: (v: WalletSortKey) => void;
  onOpen: (address: string) => void;
}) {
  const { data, loading, error, updatedAt, retry } = useApi(useRadarWallets(lifecycle, sort));
  const status = useRadarStatus().data?.data;
  const items = data?.items ?? [];
  return (
    <Panel
      title="Wallet registry"
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={RADAR_LIST_STALE_AFTER_MS} /> : null}
    >
      <div className="flex flex-wrap items-center gap-2 border-b border-line-subtle px-3 py-2">
        <label className="flex items-center gap-2">
          <span className="text-xs text-fg-muted">Lifecycle</span>
          <Select
            aria-label="Filter wallets by lifecycle"
            value={lifecycle ?? ''}
            onChange={(e) => {
              onLifecycle(e.target.value || null);
            }}
            className="h-7 w-36 text-xs sm:w-44"
          >
            <option value="">All states</option>
            {LIFECYCLE_ORDER.map((k) => (
              <option key={k} value={k}>
                {lifecycleLabel(k).label}
              </option>
            ))}
          </Select>
        </label>
        <label className="flex items-center gap-2">
          <span className="text-xs text-fg-muted">Sort</span>
          <Select
            aria-label="Sort wallets"
            value={sort}
            onChange={(e) => {
              onSort(e.target.value as WalletSortKey);
            }}
            className="h-7 w-28 text-xs"
          >
            {SORTS.map((o) => (
              <option key={o.key} value={o.key}>
                {o.label}
              </option>
            ))}
          </Select>
        </label>
      </div>
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Wallet registry" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title={
              lifecycle
                ? `No wallets in ${lifecycleLabel(lifecycle).label.toLowerCase()}.`
                : 'No wallets discovered yet.'
            }
            detail={
              lifecycle
                ? undefined
                : status?.last_cycle
                  ? 'Wallets are recorded only from verified swaps in sampled pools. Discovery has run; none were found yet.'
                  : 'No discovery cycle has run. Wallets are recorded only from verified swaps in sampled pools; request a cycle with solprint radar cycle.'
            }
          />
        </div>
      ) : (
        <>
          <Table label="Wallet registry">
            <THead>
              <Tr>
                <Th>Wallet · eligibility</Th>
                <Th numeric>Priority</Th>
                <Th className="whitespace-normal">Lifecycle</Th>
                <Th className="hidden lg:table-cell">Qualification</Th>
                <Th className="hidden md:table-cell">History · coverage</Th>
                <Th numeric className="hidden sm:table-cell">
                  Verified swaps
                </Th>
                <Th className="hidden xl:table-cell">Last trade (UTC)</Th>
                <Th className="hidden 2xl:table-cell">Monitoring</Th>
              </Tr>
            </THead>
            <TBody>
              {loading || !data ? (
                <RowsSkeleton cols={8} rows={8} />
              ) : (
                items.map((w) => <WalletRow key={w.address} w={w} onOpen={onOpen} />)
              )}
            </TBody>
          </Table>
          <p className="m-0 border-t border-line-subtle px-3 py-2 text-xs text-fg-muted">
            Priority (0–100) orders RADAR&apos;s own investigation and monitoring work. It is not a
            prediction, a recommendation or a measure of skill. Wallets are identified by address
            only, from verified swaps where the wallet is the proven owner or authority.
          </p>
        </>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- markets

function MarketRow({ m }: { m: RadarMarketDto }) {
  const sel = selectionLabel(m.selection_state);
  const supported = m.pools.filter((p) => p.validation === 'SUPPORTED');
  const top = supported[0] ?? m.pools[0] ?? null;
  const pl = top ? poolLabel(top.validation) : null;
  return (
    <Tr>
      <Td>
        <span className="text-fg">{m.symbol ?? muted('no symbol')}</span> <Id value={m.mint} />
        <span className="block text-xs text-fg-muted">
          {radarName(m.classification)} · {radarName(m.bucket)}
          {m.promotional_only ? ' · promoted only' : ''}
        </span>
      </Td>
      <Td className="whitespace-normal">
        <StatusIndicator tone={sel.tone} label={sel.label} className="whitespace-normal" />
        {m.selection_reasons[0] ? (
          <span className="block text-xs text-fg-muted">{radarName(m.selection_reasons[0])}</span>
        ) : null}
      </Td>
      <Td className="hidden md:table-cell">
        {pl && top ? (
          <>
            <StatusIndicator tone={pl.tone} label={pl.label} />
            <span className="block text-xs text-fg-muted">
              {top.protocol ? radarName(top.protocol) : (top.dex_id ?? 'unknown venue')}
              {m.pools.length > 1 ? ` · ${n(m.pools.length)} pools` : ''}
            </span>
          </>
        ) : (
          muted('No pool')
        )}
      </Td>
      <Td numeric className="hidden lg:table-cell">
        {top?.liquidity_usd ? formatDecimalString(top.liquidity_usd, 0) : '—'}
        <span className="block text-fg-muted">
          {top?.txns_1h !== null && top?.txns_1h !== undefined ? `${n(top.txns_1h)} tx/h` : '—'}
        </span>
      </Td>
      <Td numeric className="hidden sm:table-cell">
        {n(m.wallets_found)}
        <span className="block text-fg-muted">{n(m.samples_completed)} samples</span>
      </Td>
      <Td className="hidden whitespace-normal xl:table-cell">
        {m.stale ? (
          <StatusIndicator tone="warning" label="Stale" className="whitespace-normal" />
        ) : null}
        {m.last_observed_at ? (
          <span className="block text-xs text-fg-secondary">
            <Timestamp iso={m.last_observed_at} />
          </span>
        ) : (
          muted('Never observed')
        )}
      </Td>
    </Tr>
  );
}

export function MarketsPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useRadarMarkets());
  const items = data?.items ?? [];
  return (
    <Panel
      title="Market universe"
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={RADAR_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Market universe" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title="No markets recorded."
            detail="Markets come from discovery listings and from locally observed swaps. None has been recorded yet."
          />
        </div>
      ) : (
        <>
          <Table label="Market universe, newest first">
            <THead>
              <Tr>
                <Th>Token · class · bucket</Th>
                <Th className="whitespace-normal">Selection</Th>
                <Th className="hidden md:table-cell">Pool</Th>
                <Th numeric className="hidden lg:table-cell">
                  Liquidity USD · activity
                </Th>
                <Th numeric className="hidden sm:table-cell">
                  Wallets found
                </Th>
                <Th className="hidden xl:table-cell">Observed (UTC)</Th>
              </Tr>
            </THead>
            <TBody>
              {loading || !data ? (
                <RowsSkeleton cols={6} rows={6} />
              ) : (
                items.map((m) => <MarketRow key={m.mint} m={m} />)
              )}
            </TBody>
          </Table>
          <p className="m-0 border-t border-line-subtle px-3 py-2 text-xs text-fg-muted">
            Liquidity and activity are provider-reported at the observation time shown, not current
            values. Pools are sampled only when their program is validated as a supported decoder.
          </p>
        </>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- monitoring

export function MonitoringPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useRadarMonitoring());
  const items = data?.items ?? [];
  return (
    <Panel
      title="Monitoring"
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={RADAR_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <LoadFailure what="Monitoring" failure={error} onRetry={retry} />
      ) : loading || !data ? (
        <div className="flex flex-col gap-2" aria-busy="true">
          {[0, 1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <DefinitionList
            items={[
              {
                term: 'Assigned',
                value: (
                  <span className="num">
                    {n(items.length)} <span className="text-fg-muted">of {n(data.capacity)}</span>
                  </span>
                ),
              },
              {
                term: 'Mode',
                wrap: true,
                value: data.live_available ? (
                  <span className="text-fg-secondary">Live subscriptions available</span>
                ) : (
                  <span className="text-fg-muted">Periodic history refresh (no live streams)</span>
                ),
              },
            ]}
          />
          {items.length === 0 ? (
            <DataState
              kind="empty"
              title="No wallet is monitored."
              detail="A wallet is monitored only after staged history qualifies it for deeper analysis."
            />
          ) : (
            <ul aria-label="Monitored wallets" className="m-0 flex list-none flex-col p-0">
              {items.map((m) => {
                const l = monitoringLabel(m.mode);
                return (
                  <li
                    key={`${m.address}:${m.started_at}`}
                    className="flex flex-wrap items-center justify-between gap-x-3 gap-y-0.5 border-t border-line-subtle py-1.5 text-xs first:border-t-0"
                  >
                    <Id value={m.address} />
                    <StatusIndicator tone={l.tone} label={l.label} />
                    <span className="w-full text-fg-muted">
                      since <Timestamp iso={m.started_at} />
                      {m.priority_at_start ? ` · priority ${formatScore(m.priority_at_start)}` : ''}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- jobs

export function JobsPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useRadarJobs());
  const items = data?.items ?? [];
  return (
    <Panel
      title="Work queue"
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={RADAR_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Work queue" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title="No RADAR work recorded."
            detail="Pool samples and history stages appear here once a discovery cycle or an investigation queues them."
          />
        </div>
      ) : (
        <Table label="RADAR work, newest first">
          <THead>
            <Tr>
              <Th>Work · subject</Th>
              <Th className="whitespace-normal">State</Th>
              <Th numeric className="hidden sm:table-cell">
                Credits est · spent
              </Th>
              <Th numeric className="hidden md:table-cell">
                Signatures
              </Th>
              <Th className="hidden lg:table-cell">Created (UTC)</Th>
            </Tr>
          </THead>
          <TBody>
            {loading || !data ? (
              <RowsSkeleton cols={5} rows={6} />
            ) : (
              items.map((j) => {
                const l = radarJobLabel(j.state);
                return (
                  <Tr key={j.job_id}>
                    <Td>
                      <span className="text-fg">{workName(j.work_class)}</span>
                      <span className="block text-xs text-fg-muted">
                        {j.subject_kind === 'POOL' ? 'pool ' : 'wallet '}
                        <Id value={j.subject} />
                        {j.origin === 'OPERATOR' ? ' · operator' : ''}
                      </span>
                    </Td>
                    <Td className="whitespace-normal">
                      <StatusIndicator
                        tone={l.tone}
                        label={l.label}
                        className="whitespace-normal"
                      />
                      {j.defer_reason || j.result_code ? (
                        <span className="block text-xs text-fg-muted">
                          {radarName(j.defer_reason ?? j.result_code ?? '')}
                        </span>
                      ) : null}
                    </Td>
                    <Td numeric className="hidden sm:table-cell">
                      {n(j.estimated_credits)} · {n(j.credits_spent)}
                    </Td>
                    <Td numeric className="hidden md:table-cell">
                      {n(j.signatures_found)}
                      <span className="text-fg-muted"> / {n(j.signature_limit)}</span>
                    </Td>
                    <Td className="hidden lg:table-cell">
                      <Timestamp iso={j.created_at} />
                    </Td>
                  </Tr>
                );
              })
            )}
          </TBody>
        </Table>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- cycles

export function CyclesPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useRadarCycles());
  const items = data?.items ?? [];
  return (
    <Panel
      title="Discovery cycles"
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={RADAR_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <LoadFailure what="Discovery cycles" failure={error} onRetry={retry} />
      ) : loading || !data ? (
        <div className="flex flex-col gap-2" aria-busy="true">
          {[0, 1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : items.length === 0 ? (
        <DataState
          kind="empty"
          title="No discovery cycle has run."
          detail="Cycles run on schedule only with autonomous discovery on; solprint radar cycle requests one."
        />
      ) : (
        <ul aria-label="Recent discovery cycles" className="m-0 flex list-none flex-col p-0">
          {items.map((c) => {
            const l = cycleLabel(c.status);
            return (
              <li
                key={c.cycle_id}
                className="flex flex-col gap-0.5 border-t border-line-subtle py-1.5 text-xs first:border-t-0"
              >
                <span className="flex flex-wrap items-center justify-between gap-x-3">
                  <StatusIndicator
                    tone={l.tone}
                    label={l.label}
                    detail={c.deferred_reason ? radarName(c.deferred_reason) : undefined}
                  />
                  <Timestamp iso={c.started_at} className="text-fg-secondary" />
                </span>
                <span className="num text-fg-muted">
                  {radarName(c.trigger)} · {n(c.observations)} observed · {n(c.markets_new)} new ·{' '}
                  {n(c.samples_scheduled)} samples · ~{n(c.credits_estimated)} credits
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- sources

export function SourcesPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useRadarSources());
  const items = data?.items ?? [];
  return (
    <Panel
      title={`Discovery sources · last ${data ? n(data.window_hours) : '24'} h`}
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={RADAR_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Discovery sources" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title="No source has run in this window."
            detail="Each discovery cycle records one run per source, including deferred and failed runs."
          />
        </div>
      ) : (
        <>
          <Table label="Discovery sources and wallet yield">
            <THead>
              <Tr>
                <Th>Source</Th>
                <Th className="whitespace-normal">Last run</Th>
                <Th numeric className="hidden md:table-cell">
                  Runs ok · deferred · failed
                </Th>
                <Th numeric className="hidden lg:table-cell">
                  Items · new markets
                </Th>
                <Th numeric className="hidden xl:table-cell">
                  Requests · credits
                </Th>
                <Th numeric className="hidden sm:table-cell">
                  Wallets · eligible · qualified
                </Th>
              </Tr>
            </THead>
            <TBody>
              {loading || !data ? (
                <RowsSkeleton cols={6} rows={5} />
              ) : (
                items.map((r) => {
                  const l = r.last_status ? sourceRunLabel(r.last_status) : null;
                  return (
                    <Tr key={r.source}>
                      <Td>{radarName(r.source)}</Td>
                      <Td className="whitespace-normal">
                        {l ? (
                          <StatusIndicator
                            tone={l.tone}
                            label={l.label}
                            className="whitespace-normal"
                          />
                        ) : (
                          muted('No runs')
                        )}
                        {r.last_run_at ? (
                          <span className="block text-xs text-fg-muted">
                            <Timestamp iso={r.last_run_at} />
                          </span>
                        ) : null}
                      </Td>
                      <Td numeric className="hidden md:table-cell">
                        {n(r.runs_ok)} · {n(r.runs_deferred)} · {n(r.runs_failed)}
                      </Td>
                      <Td numeric className="hidden lg:table-cell">
                        {n(r.items)} · {n(r.markets_new)}
                      </Td>
                      <Td numeric className="hidden xl:table-cell">
                        {n(r.provider_requests)} · {n(r.credits_estimated)}
                      </Td>
                      <Td numeric className="hidden sm:table-cell">
                        {n(r.unique_wallets)} · {n(r.eligible_wallets)} · {n(r.qualified_wallets)}
                      </Td>
                    </Tr>
                  );
                })
              )}
            </TBody>
          </Table>
          <p className="m-0 border-t border-line-subtle px-3 py-2 text-xs text-fg-muted">
            Yield is attributed to the source that first led to each wallet&apos;s evidence. Credits
            are GATEWAY estimates recorded at request time.
          </p>
        </>
      )}
    </Panel>
  );
}
