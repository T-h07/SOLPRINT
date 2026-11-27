/**
 * DATA ENGINE workspace panes. Every value comes from the backend's
 * PostgreSQL read models or in-process counters; polling these endpoints
 * never sends a provider request and never queues work. Raw amounts are
 * strings formatted with BigInt; nothing here estimates or fills in values
 * the backend did not record.
 */
import type { ReactNode } from 'react';

import { DataState, Skeleton } from '@/components/ui/DataState.tsx';
import { Select } from '@/components/ui/Field.tsx';
import { MetricCell, Timestamp } from '@/components/ui/Metric.tsx';
import { DefinitionList, Panel } from '@/components/ui/Panel.tsx';
import { StatusIndicator } from '@/components/ui/StatusIndicator.tsx';
import { TBody, THead, Table, Td, Th, Tr } from '@/components/ui/Table.tsx';
import {
  DATA_LIST_STALE_AFTER_MS,
  useDataBackfills,
  useDataEvents,
  useDataIssues,
  useDataStatus,
  useDataSwaps,
  useDataTargets,
  useDataTransactions,
} from '@/lib/api/queries.ts';
import {
  formatBytes,
  formatDecimalString,
  formatInteger,
  formatLamportsAsSol,
  formatTokenAmount,
  shortId,
} from '@/lib/format/number.ts';
import type { LabelCount, SwapAssetDto, SwapDto, TransactionSummaryDto } from '@/types/api.ts';

import { Age, LoadFailure, RowsSkeleton } from '../shared/pane.tsx';
import { useApi } from '../shared/useApi.ts';
import {
  attributionLabel,
  backfillLabel,
  coverageLabel,
  decodeLabel,
  finalityLabel,
  humanize,
  JOB_ORDER,
  jobLabel,
  PROTOCOLS,
  protocolName,
  SEVERITY_ORDER,
  severityLabel,
} from './labels.ts';

const n = (v: number) => formatInteger(v);
const WSOL = 'So11111111111111111111111111111111111111112';

const count = (list: LabelCount[] | undefined, label: string) =>
  list?.find((c) => c.label === label)?.count ?? 0;

/** Monospace identifier, shortened; the full value is in the accessible name and title. */
export function Id({ value, keep = 4 }: { value: string; keep?: number }) {
  return (
    <span className="font-mono text-xs" title={value} aria-label={value}>
      {shortId(value, keep)}
    </span>
  );
}

function assetName(a: SwapAssetDto): ReactNode {
  if (a.asset_kind === 'NATIVE_SOL' || a.mint === WSOL) {
    return <span className="text-fg-secondary">SOL</span>;
  }
  return a.mint ? <Id value={a.mint} /> : <span className="text-fg-muted">unknown</span>;
}

/** "1,234.5678 SOL" with BigInt; unknown amounts are "—", never 0. */
export function Amount({ asset }: { asset: SwapAssetDto }) {
  return (
    <span className="inline-flex items-baseline gap-1.5">
      <span className="num">{formatTokenAmount(asset.amount, asset.decimals)}</span>
      {assetName(asset)}
    </span>
  );
}

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

// ---------------------------------------------------------------- engine state

export function EngineStatePanel({ className }: { className?: string }) {
  const { data: s, loading, error, updatedAt, retry } = useApi(useDataStatus());
  const p = s?.process ?? null;
  const cfg = s?.settings ?? null;
  return (
    <Panel title="Data engine" meta={s ? <Age at={updatedAt} /> : null} className={className}>
      {error && !s ? (
        <LoadFailure what="Data engine state" failure={error} onRetry={retry} />
      ) : loading || !s ? (
        <div className="flex flex-col gap-2" aria-busy="true">
          {[0, 1, 2, 3, 4, 5].map((i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : (
        <DefinitionList
          items={[
            {
              term: 'Engine',
              wrap: true,
              value: s.running ? (
                <StatusIndicator tone="positive" label="Running" />
              ) : (
                <StatusIndicator tone="neutral" label="Not running" detail="stored data only" />
              ),
            },
            {
              term: 'Execution',
              value: <span className="text-fg-secondary">None · read-only decoding</span>,
            },
            {
              term: 'Decoder',
              value: <span className="font-mono text-xs">{s.decoder_version}</span>,
            },
            {
              term: 'Live intake',
              wrap: true,
              value: cfg?.auto_ingest_enabled ? (
                <StatusIndicator
                  tone="info"
                  label="On"
                  detail="enabled targets via GATEWAY streams"
                />
              ) : (
                <StatusIndicator tone="neutral" label="Off" detail="manual and backfill only" />
              ),
            },
            {
              term: 'Backfill',
              value: cfg?.backfill_enabled ? (
                <span className="text-fg-secondary">Allowed · bounded per run</span>
              ) : (
                <span className="text-fg-muted">Disabled</span>
              ),
            },
            {
              term: 'USD valuation',
              wrap: true,
              value: cfg?.price_enrichment_enabled ? (
                <span className="text-fg-secondary">Enrichment on</span>
              ) : (
                <span className="text-fg-muted">Off · swaps carry no USD value</span>
              ),
            },
            {
              term: 'Commitment',
              value: <span className="text-fg-secondary">{cfg?.default_commitment ?? '—'}</span>,
            },
            {
              term: 'Since start',
              wrap: true,
              value: p ? (
                <span className="num text-fg-secondary">
                  {n(p.persisted)} decoded · {n(p.unchanged)} unchanged · {n(p.retries)} retries ·{' '}
                  {n(p.credits_charged)} credits
                </span>
              ) : (
                <span className="text-fg-muted">No process counters (engine not running)</span>
              ),
            },
            {
              term: 'Last error',
              wrap: true,
              value:
                p?.last_error_code && p.last_error_at ? (
                  <span className="text-fg-secondary">
                    <span className="font-mono text-xs">{p.last_error_code}</span> ·{' '}
                    <Timestamp iso={p.last_error_at} />
                  </span>
                ) : (
                  <span className="text-fg-muted">None recorded</span>
                ),
            },
          ]}
        />
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- ingestion

export function IngestionPanel({ className }: { className?: string }) {
  const { data: s, loading, error, updatedAt, retry } = useApi(useDataStatus());
  const unavailable = s && !s.counts_available ? 'database unavailable' : undefined;
  const q = s?.queue ?? null;
  const t = s?.transactions ?? null;
  const w = s?.swaps ?? null;
  const blocking =
    count(s?.open_issues_by_severity, 'CRITICAL') + count(s?.open_issues_by_severity, 'INVALID');
  return (
    <Panel title="Ingestion" meta={s ? <Age at={updatedAt} /> : null} className={className}>
      {error && !s ? (
        <LoadFailure what="Ingestion totals" failure={error} onRetry={retry} />
      ) : (
        <div className="flex flex-col gap-4">
          <div className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4">
            <MetricCell
              size="lg"
              label={q ? `Queue · of ${n(q.capacity)}` : 'Queue'}
              loading={loading}
              unavailable={unavailable}
              value={q ? n(q.depth) : null}
            />
            <MetricCell
              label="Transactions"
              loading={loading}
              unavailable={unavailable}
              value={t ? n(t.total) : null}
            />
            <MetricCell
              label="Swaps · verified"
              loading={loading}
              unavailable={unavailable}
              value={w ? `${n(w.canonical)} · ${n(w.verified)}` : null}
            />
            <MetricCell
              label="Blocking issues"
              loading={loading}
              unavailable={unavailable}
              value={
                s?.counts_available ? (
                  blocking > 0 ? (
                    <StatusIndicator
                      tone="negative"
                      label={n(blocking)}
                      detail="critical or invalid"
                    />
                  ) : (
                    n(0)
                  )
                ) : null
              }
            />
          </div>
          {loading ? <Skeleton className="h-24 w-full" /> : null}
          {s?.counts_available && q && t && w ? (
            <DefinitionList
              items={[
                {
                  term: 'Jobs',
                  wrap: true,
                  value: (
                    <Breakdown
                      label="Jobs by state"
                      items={JOB_ORDER.filter((k) => count(q.by_state, k) > 0).map((k) => ({
                        key: k,
                        node: (
                          <>
                            <span className="text-fg-muted">{jobLabel(k).label}</span>{' '}
                            {n(count(q.by_state, k))}
                          </>
                        ),
                      }))}
                    />
                  ),
                },
                {
                  term: 'Oldest pending',
                  value: q.oldest_pending_at ? (
                    <Timestamp iso={q.oldest_pending_at} />
                  ) : (
                    <span className="text-fg-muted">Nothing pending</span>
                  ),
                },
                {
                  term: 'Finality',
                  wrap: true,
                  value: (
                    <Breakdown
                      label="Transactions by finality"
                      items={t.by_finality.map((c) => ({
                        key: c.label,
                        node: (
                          <>
                            <span className="text-fg-muted">{finalityLabel(c.label).label}</span>{' '}
                            {n(c.count)}
                          </>
                        ),
                      }))}
                    />
                  ),
                },
                {
                  term: 'Decode status',
                  wrap: true,
                  value: (
                    <Breakdown
                      label="Transactions by decode status"
                      items={t.by_decode_status.map((c) => ({
                        key: c.label,
                        node: (
                          <>
                            <span className="text-fg-muted">{decodeLabel(c.label).label}</span>{' '}
                            {n(c.count)}
                          </>
                        ),
                      }))}
                    />
                  ),
                },
                {
                  term: 'Last decoded',
                  value: t.last_decoded_at ? (
                    <Timestamp iso={t.last_decoded_at} />
                  ) : (
                    <span className="text-fg-muted">Never</span>
                  ),
                },
                {
                  term: 'Evidence',
                  wrap: true,
                  value: s.evidence ? (
                    <span className="num text-fg-secondary">
                      {formatBytes(s.evidence.bytes)} stored · {n(s.evidence.pruned)} pruned · kept{' '}
                      {n(s.evidence.retention_days)} days
                    </span>
                  ) : (
                    '—'
                  ),
                },
                {
                  term: 'Outbox',
                  value: s.outbox ? (
                    <span className="num text-fg-secondary">
                      {n(s.outbox.events)} events · {n(s.outbox.undelivered)} undelivered
                    </span>
                  ) : (
                    '—'
                  ),
                },
              ]}
            />
          ) : s && !s.counts_available ? (
            <DataState
              kind="partial"
              title="Stored totals unavailable: PostgreSQL could not be read."
              detail="Engine state above is still current. Nothing is shown as zero."
            />
          ) : null}
        </div>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- swaps

function SwapRow({ s, onOpen }: { s: SwapDto; onOpen: (sig: string) => void }) {
  const a = attributionLabel(s.attribution);
  const st = decodeLabel(s.classification_status);
  const f = finalityLabel(s.finality);
  return (
    <Tr>
      <Td>
        <button
          type="button"
          onClick={() => {
            onOpen(s.signature);
          }}
          className="rounded-xs text-left text-fg underline decoration-line-control underline-offset-2 hover:decoration-fg"
          title={s.signature}
        >
          <span className="sr-only">Inspect transaction at </span>
          {s.block_time ? (
            <Timestamp iso={s.block_time} />
          ) : (
            <span className="num">slot {n(s.slot)}</span>
          )}
        </button>
        <span className="block text-xs text-fg-muted md:hidden">{protocolName(s.protocol)}</span>
        <span className="mt-0.5 flex flex-col text-xs sm:hidden">
          <span>
            <span className="text-fg-muted">sent </span>
            <Amount asset={s.input} />
          </span>
          <span>
            <span className="text-fg-muted">received </span>
            <Amount asset={s.output} />
          </span>
        </span>
      </Td>
      <Td className="hidden md:table-cell">
        {protocolName(s.protocol)}
        {s.aggregator ? null : s.legs.length > 1 ? (
          <span className="num text-fg-muted"> · {n(s.legs.length)} legs</span>
        ) : null}
      </Td>
      <Td className="hidden lg:table-cell">
        {s.wallet ? <Id value={s.wallet} /> : <span className="text-fg-muted">—</span>}
        <span className="block">
          <StatusIndicator tone={a.tone} label={a.label} />
        </span>
      </Td>
      <Td numeric className="hidden sm:table-cell">
        <Amount asset={s.input} />
      </Td>
      <Td numeric className="hidden sm:table-cell">
        <Amount asset={s.output} />
      </Td>
      <Td numeric className="hidden 2xl:table-cell">
        {s.exchange_ratio ? formatDecimalString(s.exchange_ratio, 6) : '—'}
      </Td>
      <Td className="whitespace-normal">
        <StatusIndicator tone={st.tone} label={st.label} className="whitespace-normal" />
        <span className="block">
          <StatusIndicator tone={f.tone} label={f.label} className="whitespace-normal" />
        </span>
        {s.canonical ? null : (
          <span
            className="block text-xs text-fg-muted"
            title={humanize(s.retraction_reason ?? 'revision')}
          >
            retracted
          </span>
        )}
      </Td>
    </Tr>
  );
}

export function SwapsPanel({
  className,
  protocol,
  onProtocol,
  onOpen,
}: {
  className?: string;
  protocol: string | null;
  onProtocol: (p: string | null) => void;
  onOpen: (sig: string) => void;
}) {
  const { data, loading, error, updatedAt, retry } = useApi(useDataSwaps(protocol));
  const items = data?.items ?? [];
  return (
    <Panel
      title="Swaps"
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={DATA_LIST_STALE_AFTER_MS} /> : null}
      actions={
        <label className="flex items-center gap-2">
          <span className="sr-only sm:not-sr-only">Protocol</span>
          <Select
            aria-label="Filter swaps by protocol"
            value={protocol ?? ''}
            onChange={(e) => {
              onProtocol(e.target.value || null);
            }}
            className="h-7 w-40 text-xs"
          >
            <option value="">All protocols</option>
            {PROTOCOLS.map((p) => (
              <option key={p} value={p}>
                {protocolName(p)}
              </option>
            ))}
          </Select>
        </label>
      }
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Swaps" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title={
              protocol ? `No ${protocolName(protocol)} swaps recorded.` : 'No swaps recorded yet.'
            }
            detail="Swaps appear once queued transactions are fetched and decoded. Queue work with `solprint data-engine ingest` or a bounded backfill."
          />
        </div>
      ) : (
        <>
          <Table label="Normalized swaps, newest first">
            <THead>
              <Tr>
                <Th>Time (UTC)</Th>
                <Th className="hidden md:table-cell">Protocol</Th>
                <Th className="hidden lg:table-cell">Wallet · attribution</Th>
                <Th numeric className="hidden sm:table-cell">
                  Input
                </Th>
                <Th numeric className="hidden sm:table-cell">
                  Output
                </Th>
                <Th numeric className="hidden 2xl:table-cell">
                  Out per in
                </Th>
                <Th className="whitespace-normal">Status · finality</Th>
              </Tr>
            </THead>
            <TBody>
              {loading || !data ? (
                <RowsSkeleton cols={7} rows={6} />
              ) : (
                items.map((s) => <SwapRow key={s.swap_id} s={s} onOpen={onOpen} />)
              )}
            </TBody>
          </Table>
          <p className="m-0 border-t border-line-subtle px-3 py-2 text-xs text-fg-muted">
            Amounts are what the wallet sent and received, from token-balance and transfer evidence.
            A wallet is shown only for verified attribution; the fee payer is never assumed to be
            the trader. No USD values are shown while price enrichment is off.
          </p>
        </>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- transactions

function TxRow({ t, onOpen }: { t: TransactionSummaryDto; onOpen: (sig: string) => void }) {
  const d = decodeLabel(t.decode_status);
  const f = finalityLabel(t.finality);
  return (
    <Tr>
      <Td>
        <button
          type="button"
          onClick={() => {
            onOpen(t.signature);
          }}
          className="rounded-xs text-left underline decoration-line-control underline-offset-2 hover:decoration-fg"
          title={t.signature}
        >
          <span className="sr-only">Inspect transaction </span>
          <span className="font-mono text-xs text-fg">{shortId(t.signature, 6)}</span>
        </button>
        {t.execution_status === 'FAILED' ? (
          <span className="block">
            <StatusIndicator tone="negative" label="Failed on chain" />
          </span>
        ) : null}
      </Td>
      <Td numeric className="hidden md:table-cell">
        {n(t.slot)}
      </Td>
      <Td className="hidden lg:table-cell">
        {t.protocols.length > 0 ? (
          t.protocols.map(protocolName).join(', ')
        ) : (
          <span className="text-fg-muted">none</span>
        )}
      </Td>
      <Td>
        <StatusIndicator tone={d.tone} label={d.label} />
      </Td>
      <Td className="hidden sm:table-cell">
        <StatusIndicator tone={f.tone} label={f.label} />
      </Td>
      <Td numeric className="hidden xl:table-cell">
        {n(t.swap_count)}
      </Td>
      <Td numeric className="hidden 2xl:table-cell">
        {formatLamportsAsSol(t.fee_lamports, 6)}
      </Td>
    </Tr>
  );
}

export function TransactionsPanel({
  className,
  onOpen,
}: {
  className?: string;
  onOpen: (sig: string) => void;
}) {
  const { data, loading, error, updatedAt, retry } = useApi(useDataTransactions());
  const items = data?.items ?? [];
  return (
    <Panel
      title="Transactions"
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={DATA_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Transactions" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState kind="empty" title="No transactions decoded yet." />
        </div>
      ) : (
        <Table label="Decoded transactions, newest first">
          <THead>
            <Tr>
              <Th>Signature</Th>
              <Th numeric className="hidden md:table-cell">
                Slot
              </Th>
              <Th className="hidden lg:table-cell">Protocols</Th>
              <Th>Decode</Th>
              <Th className="hidden sm:table-cell">Finality</Th>
              <Th numeric className="hidden xl:table-cell">
                Swaps
              </Th>
              <Th numeric className="hidden 2xl:table-cell">
                Fee (SOL)
              </Th>
            </Tr>
          </THead>
          <TBody>
            {loading || !data ? (
              <RowsSkeleton cols={7} rows={5} />
            ) : (
              items.map((t) => <TxRow key={t.signature} t={t} onOpen={onOpen} />)
            )}
          </TBody>
        </Table>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- quality

export function QualityPanel({ className }: { className?: string }) {
  const status = useApi(useDataStatus());
  const { data, loading, error, updatedAt, retry } = useApi(useDataIssues());
  const bySeverity = status.data?.open_issues_by_severity ?? [];
  const items = data?.items ?? [];
  return (
    <Panel
      title="Data quality"
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={DATA_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Quality issues" failure={error} onRetry={retry} />
        </div>
      ) : (
        <>
          <div className="border-b border-line-subtle px-3 py-2">
            <Breakdown
              label="Open issues by severity"
              items={SEVERITY_ORDER.map((sv) => {
                const l = severityLabel(sv);
                const c = count(bySeverity, sv);
                return {
                  key: sv,
                  node:
                    c > 0 ? (
                      <StatusIndicator tone={l.tone} label={l.label} detail={n(c)} />
                    ) : (
                      <span className="text-fg-muted">
                        {l.label} {n(0)}
                      </span>
                    ),
                };
              })}
            />
          </div>
          {data && items.length === 0 ? (
            <div className="p-3">
              <DataState kind="empty" title="No open quality issues." />
            </div>
          ) : (
            <Table label="Open data-quality issues">
              <THead>
                <Tr>
                  <Th>Issue</Th>
                  <Th>Severity</Th>
                  <Th numeric className="hidden sm:table-cell">
                    Seen
                  </Th>
                </Tr>
              </THead>
              <TBody>
                {loading || !data ? (
                  <RowsSkeleton cols={3} />
                ) : (
                  items.map((i) => {
                    const l = severityLabel(i.severity);
                    return (
                      <Tr key={i.issue_id}>
                        <Td className="whitespace-normal">
                          <span className="font-mono text-xs break-all text-fg">{i.code}</span>
                          {i.detail ? (
                            <span
                              className="block max-w-80 text-xs break-words text-fg-muted"
                              title={i.detail}
                            >
                              {i.detail}
                            </span>
                          ) : null}
                        </Td>
                        <Td>
                          <StatusIndicator tone={l.tone} label={l.label} />
                        </Td>
                        <Td numeric className="hidden sm:table-cell">
                          {n(i.occurrences)}×
                        </Td>
                      </Tr>
                    );
                  })
                )}
              </TBody>
            </Table>
          )}
        </>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------- targets and backfills

export function TargetsPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useDataTargets());
  const items = data?.items ?? [];
  const enabled = items.filter((t) => t.enabled).length;
  return (
    <Panel
      title="Targets"
      flush
      className={className}
      meta={
        data ? (
          <>
            <span className="num">
              {n(enabled)} enabled · limit {n(data.max_active_targets)}
            </span>
            <Age at={updatedAt} staleAfterMs={DATA_LIST_STALE_AFTER_MS} />
          </>
        ) : null
      }
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Targets" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title="No ingestion targets configured."
            detail="Targets are added explicitly by the owner (`solprint data-engine targets add`); none are created automatically."
          />
        </div>
      ) : (
        <Table label="Ingestion targets and coverage">
          <THead>
            <Tr>
              <Th>Address</Th>
              <Th>State</Th>
              <Th className="hidden md:table-cell">Coverage</Th>
              <Th className="hidden lg:table-cell">Priority</Th>
              <Th numeric className="hidden xl:table-cell">
                Last observation
              </Th>
            </Tr>
          </THead>
          <TBody>
            {loading || !data ? (
              <RowsSkeleton cols={5} rows={3} />
            ) : (
              items.map((t) => {
                const c = coverageLabel(t.coverage_state);
                return (
                  <Tr key={t.target_id}>
                    <Td>
                      <Id value={t.address} />
                      {t.label ? (
                        <span className="ml-2 text-xs text-fg-secondary">{t.label}</span>
                      ) : null}
                    </Td>
                    <Td>
                      {t.enabled ? (
                        <StatusIndicator tone="positive" label="Enabled" />
                      ) : (
                        <StatusIndicator tone="neutral" label="Disabled" />
                      )}
                    </Td>
                    <Td className="hidden md:table-cell">
                      <StatusIndicator tone={c.tone} label={c.label} />
                    </Td>
                    <Td className="hidden lg:table-cell">
                      <span className="num">{t.priority}</span>{' '}
                      <span className="text-fg-muted">· {t.commitment}</span>
                    </Td>
                    <Td numeric className="hidden text-fg-muted xl:table-cell">
                      {t.last_observation_at ? <Timestamp iso={t.last_observation_at} /> : 'Never'}
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

export function BackfillsPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useDataBackfills());
  const items = data?.items ?? [];
  return (
    <Panel
      title="Backfills"
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={DATA_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Backfills" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState
            kind="empty"
            title="No backfills requested."
            detail="Backfills are bounded by pages and signatures and run only when requested."
          />
        </div>
      ) : (
        <Table label="Bounded historical backfills">
          <THead>
            <Tr>
              <Th>Address</Th>
              <Th>Status</Th>
              <Th numeric className="hidden sm:table-cell">
                Pages
              </Th>
              <Th numeric className="hidden md:table-cell">
                Queued · dup
              </Th>
              <Th numeric className="hidden lg:table-cell">
                Credits
              </Th>
            </Tr>
          </THead>
          <TBody>
            {loading || !data ? (
              <RowsSkeleton cols={5} rows={2} />
            ) : (
              items.map((b) => {
                const l = backfillLabel(b.status);
                const c = coverageLabel(b.coverage_state);
                return (
                  <Tr key={b.backfill_id}>
                    <Td>
                      <Id value={b.address} />
                      <span className="block text-xs text-fg-muted">{c.label}</span>
                    </Td>
                    <Td>
                      <StatusIndicator tone={l.tone} label={l.label} />
                      {b.stop_reason ? (
                        <span className="block font-mono text-2xs text-fg-muted">
                          {b.stop_reason}
                        </span>
                      ) : null}
                    </Td>
                    <Td numeric className="hidden sm:table-cell">
                      {n(b.pages_fetched)} / {n(b.max_pages)}
                    </Td>
                    <Td numeric className="hidden md:table-cell">
                      {n(b.queued_count)} · {n(b.duplicate_count)}
                    </Td>
                    <Td numeric className="hidden lg:table-cell">
                      {n(b.credits_spent)}{' '}
                      <span className="text-fg-muted">/ est. {n(b.credits_estimated)}</span>
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

// ---------------------------------------------------------------- events

export function EventsPanel({ className }: { className?: string }) {
  const { data, loading, error, updatedAt, retry } = useApi(useDataEvents());
  const items = data?.items ?? [];
  return (
    <Panel
      title="Domain events"
      flush
      className={className}
      meta={data ? <Age at={updatedAt} staleAfterMs={DATA_LIST_STALE_AFTER_MS} /> : null}
    >
      {error && !data ? (
        <div className="p-3">
          <LoadFailure what="Domain events" failure={error} onRetry={retry} />
        </div>
      ) : data && items.length === 0 ? (
        <div className="p-3">
          <DataState kind="empty" title="No domain events persisted yet." />
        </div>
      ) : (
        <Table label="Recent domain events from the transactional outbox">
          <THead>
            <Tr>
              <Th numeric>Seq</Th>
              <Th>Event</Th>
              <Th className="hidden md:table-cell">Aggregate</Th>
              <Th numeric className="hidden lg:table-cell">
                Persisted
              </Th>
            </Tr>
          </THead>
          <TBody>
            {loading || !data ? (
              <RowsSkeleton cols={4} rows={5} />
            ) : (
              items.map((e) => (
                <Tr key={e.event_id}>
                  <Td numeric>{n(e.seq)}</Td>
                  <Td>
                    <span className="text-fg">{e.event_type}</span>
                    <span className="block text-xs text-fg-muted">{humanize(e.quality_state)}</span>
                  </Td>
                  <Td className="hidden md:table-cell">
                    <span className="text-xs text-fg-muted">{e.aggregate_type.toLowerCase()} </span>
                    <span className="font-mono text-xs" title={e.aggregate_id}>
                      {shortId(e.aggregate_id, 6)}
                    </span>
                    <span className="num text-xs text-fg-muted"> r{n(e.aggregate_revision)}</span>
                  </Td>
                  <Td numeric className="hidden text-fg-muted lg:table-cell">
                    <Timestamp iso={e.persisted_at} />
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
