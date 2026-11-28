/**
 * Wallet inspector: why RADAR holds a wallet where it is — priority
 * components, matched outcomes, activity windows, history coverage,
 * lifecycle transitions and the verified swaps that are its evidence.
 * Read-only; changes are `solprint radar` CLI operations.
 */
import type { ReactNode } from 'react';

import { DataState, Skeleton } from '@/components/ui/DataState.tsx';
import { Drawer } from '@/components/ui/Drawer.tsx';
import { Timestamp } from '@/components/ui/Metric.tsx';
import { DefinitionList } from '@/components/ui/Panel.tsx';
import { StatusIndicator } from '@/components/ui/StatusIndicator.tsx';
import { TBody, THead, Table, Td, Th, Tr } from '@/components/ui/Table.tsx';
import { finalityLabel, protocolName } from '@/features/data/labels.ts';
import { Id } from '@/features/data/panels.tsx';
import {
  useRadarWallet,
  useRadarWalletEvidence,
  useRadarWalletHistory,
} from '@/lib/api/queries.ts';
import {
  formatBpsAsPercent,
  formatDecimalString,
  formatInteger,
  formatTokenAmount,
} from '@/lib/format/number.ts';
import type { RadarDenominationDto, RadarWalletDetail } from '@/types/api.ts';

import { LoadFailure } from '../shared/pane.tsx';
import { useApi } from '../shared/useApi.ts';
import {
  eligibilityLabel,
  formatComponent,
  formatScore,
  lifecycleLabel,
  monitoringLabel,
  qualificationLabel,
  radarCoverageLabel,
  radarJobLabel,
  radarName,
  workName,
} from './labels.ts';

const n = (v: number) => formatInteger(v);

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2 border-t border-line pt-3">
      <h3 className="text-sm font-medium text-fg">{title}</h3>
      {children}
    </section>
  );
}

const QUOTE_DECIMALS: Record<string, number> = { SOL: 9, USDC: 6, USDT: 6 };

function Denomination({ d }: { d: RadarDenominationDto }) {
  const dec = QUOTE_DECIMALS[d.denomination] ?? null;
  return (
    <Tr>
      <Td>{d.denomination}</Td>
      <Td numeric>
        {n(d.outcomes)}
        <span className="text-fg-muted"> · {n(d.wins)} above cost</span>
      </Td>
      <Td numeric>{formatTokenAmount(d.cost, dec)}</Td>
      <Td numeric>{formatTokenAmount(d.proceeds, dec)}</Td>
      <Td numeric>{d.return_ratio ? `${formatDecimalString(d.return_ratio, 4)}×` : '—'}</Td>
    </Tr>
  );
}

const WEIGHTS: {
  key: 'activity' | 'recency' | 'evidence_quality' | 'performance' | 'diversity';
  label: string;
  w: string;
}[] = [
  { key: 'activity', label: 'Activity (A)', w: '0.25' },
  { key: 'recency', label: 'Recency (R)', w: '0.15' },
  { key: 'evidence_quality', label: 'Evidence quality (E)', w: '0.25' },
  { key: 'performance', label: 'Matched performance (P)', w: '0.25' },
  { key: 'diversity', label: 'Token diversity (D)', w: '0.10' },
];

function Detail({ d, address }: { d: RadarWalletDetail; address: string }) {
  const w = d.wallet;
  const m = d.metrics;
  const l = lifecycleLabel(w.lifecycle);
  const e = eligibilityLabel(w.eligibility);
  const c = radarCoverageLabel(w.coverage_state);
  const mon = monitoringLabel(w.monitoring_mode);
  const history = useApi(useRadarWalletHistory(address));
  const evidence = useApi(useRadarWalletEvidence(address));
  return (
    <div className="flex flex-col gap-3">
      <DefinitionList
        items={[
          {
            term: 'Address',
            wrap: true,
            value: <span className="font-mono text-xs">{w.address}</span>,
          },
          { term: 'Lifecycle', value: <StatusIndicator tone={l.tone} label={l.label} /> },
          {
            term: 'Eligibility',
            wrap: true,
            value: (
              <span>
                <StatusIndicator tone={e.tone} label={e.label} />
                {w.operator_excluded ? (
                  <span className="text-xs text-fg-muted"> · excluded by operator</span>
                ) : null}
              </span>
            ),
          },
          {
            term: 'Qualification',
            wrap: true,
            value: w.qualification ? (
              <span className="text-fg-secondary">{qualificationLabel(w.qualification).label}</span>
            ) : (
              <span className="text-fg-muted">Not evaluated</span>
            ),
          },
          {
            term: 'Priority',
            value: (
              <span className="num">
                {formatScore(w.priority_score)}
                <span className="text-xs text-fg-muted">
                  {' '}
                  / 100 · {w.priority_policy ?? 'no policy'}
                </span>
              </span>
            ),
          },
          {
            term: 'History',
            wrap: true,
            value: (
              <span className="inline-flex flex-wrap items-center gap-x-2">
                <span className="num text-fg-secondary">
                  {w.history_stage === 'NONE' ? 'No stage complete' : `Stage ${w.history_stage}`}
                </span>
                <StatusIndicator tone={c.tone} label={c.label} />
              </span>
            ),
          },
          {
            term: 'Covered',
            wrap: true,
            value:
              w.coverage_start && w.coverage_end ? (
                <span className="text-fg-secondary">
                  <Timestamp iso={w.coverage_start} /> – <Timestamp iso={w.coverage_end} />
                </span>
              ) : (
                <span className="text-fg-muted">No coverage window recorded</span>
              ),
          },
          { term: 'Monitoring', value: <StatusIndicator tone={mon.tone} label={mon.label} /> },
          {
            term: 'Discovered',
            wrap: true,
            value: (
              <span className="text-fg-secondary">
                <Timestamp iso={w.first_discovered_at} /> · {radarName(w.first_discovery_source)}
                {d.discovery_sources.length > 1
                  ? ` · also ${d.discovery_sources
                      .filter((s) => s !== w.first_discovery_source)
                      .map(radarName)
                      .join(', ')}`
                  : ''}
              </span>
            ),
          },
        ]}
      />

      <Section title="Priority components">
        {m ? (
          <>
            <Table label="Priority components" density="compact">
              <THead>
                <Tr>
                  <Th>Component</Th>
                  <Th numeric>Weight</Th>
                  <Th numeric>Value (0–100)</Th>
                </Tr>
              </THead>
              <TBody>
                {WEIGHTS.map((c) => (
                  <Tr key={c.key}>
                    <Td>{c.label}</Td>
                    <Td numeric>{c.w}</Td>
                    <Td numeric>{formatComponent(m.priority[c.key])}</Td>
                  </Tr>
                ))}
              </TBody>
            </Table>
            <p className="m-0 text-xs text-fg-muted">
              Score = 100 × Σ weight × component. P is shrunk toward zero with few matched outcomes
              (n / (n + 10)); status: {radarName(m.priority.performance_status)}. Computed{' '}
              <Timestamp iso={m.computed_at} /> under {m.policy}.
            </p>
          </>
        ) : (
          <DataState kind="empty" title="No evaluation snapshot yet." />
        )}
      </Section>

      {m ? (
        <Section title="Matched outcomes (FIFO, fees included)">
          <span className="num text-xs text-fg-secondary">
            {n(m.screening.outcomes)} outcomes · {n(m.screening.disposals_matched)} of{' '}
            {n(m.screening.disposals)} disposals matched (
            {formatBpsAsPercent(m.screening.matched_coverage_bps).replace('+', '')}) ·{' '}
            {n(m.screening.open_positions)} open · {n(m.screening.unknown_cost_tokens)} unknown cost
          </span>
          {m.screening.by_denomination.length > 0 ? (
            <Table label="Matched outcomes by quote denomination">
              <THead>
                <Tr>
                  <Th>Quote</Th>
                  <Th numeric>Outcomes</Th>
                  <Th numeric>Cost</Th>
                  <Th numeric>Proceeds</Th>
                  <Th numeric>Ratio</Th>
                </Tr>
              </THead>
              <TBody>
                {m.screening.by_denomination.map((x) => (
                  <Denomination key={x.denomination} d={x} />
                ))}
              </TBody>
            </Table>
          ) : (
            <span className="text-xs text-fg-muted">No matched outcome yet.</span>
          )}
          <p className="m-0 text-xs text-fg-muted">
            Ratios are proceeds over cost in the same quote asset; denominations are never mixed or
            converted, and no USD value is implied.
          </p>
        </Section>
      ) : null}

      {m ? (
        <Section title="Activity">
          <Table label="Activity windows">
            <THead>
              <Tr>
                <Th>Window</Th>
                <Th numeric>Swaps</Th>
                <Th numeric>Buys · sells</Th>
                <Th numeric>Tokens</Th>
                <Th numeric>Days</Th>
              </Tr>
            </THead>
            <TBody>
              {m.activity.map((a) => (
                <Tr key={a.window}>
                  <Td>{a.window}</Td>
                  <Td numeric>{n(a.swaps)}</Td>
                  <Td numeric>
                    {n(a.buys)} · {n(a.sells)}
                  </Td>
                  <Td numeric>{n(a.distinct_tokens)}</Td>
                  <Td numeric>{n(a.trading_days)}</Td>
                </Tr>
              ))}
            </TBody>
          </Table>
        </Section>
      ) : null}

      <Section title="Retrieval">
        <span className="num text-xs text-fg-secondary">
          {n(d.retrieval.signatures_discovered)} signatures ·{' '}
          {n(d.retrieval.transactions_retrieved)} retrieved · {n(d.retrieval.transactions_pending)}{' '}
          pending · {n(d.retrieval.transactions_unavailable)} unavailable ·{' '}
          {n(d.retrieval.credits_spent)} credits
          {d.retrieval.history_exhausted ? ' · address history exhausted' : ''}
        </span>
        {d.recent_jobs.length > 0 ? (
          <ul
            aria-label="Recent RADAR work"
            className="m-0 flex list-none flex-col gap-1 p-0 text-xs"
          >
            {d.recent_jobs.slice(0, 6).map((j) => {
              const jl = radarJobLabel(j.state);
              return (
                <li key={j.job_id} className="flex flex-wrap items-center gap-x-2">
                  <span className="text-fg">{workName(j.work_class)}</span>
                  <StatusIndicator tone={jl.tone} label={jl.label} />
                  {j.result_code || j.defer_reason ? (
                    <span className="text-fg-muted">
                      {radarName(j.result_code ?? j.defer_reason ?? '')}
                    </span>
                  ) : null}
                </li>
              );
            })}
          </ul>
        ) : null}
      </Section>

      <Section title="Lifecycle history">
        {history.error && !history.data ? (
          <LoadFailure what="Lifecycle history" failure={history.error} onRetry={history.retry} />
        ) : !history.data ? (
          <Skeleton className="h-16 w-full" />
        ) : history.data.items.length === 0 ? (
          <span className="text-xs text-fg-muted">No transition recorded.</span>
        ) : (
          <ol
            aria-label="Lifecycle transitions, newest first"
            className="m-0 flex list-none flex-col gap-1 p-0 text-xs"
          >
            {history.data.items.map((t) => (
              <li key={`${t.transitioned_at}:${t.to_state}`} className="flex flex-col">
                <span className="flex flex-wrap items-center gap-x-2">
                  <Timestamp iso={t.transitioned_at} className="text-fg-secondary" />
                  <span className="text-fg">
                    {t.from_state ? `${lifecycleLabel(t.from_state).label} → ` : ''}
                    {lifecycleLabel(t.to_state).label}
                  </span>
                </span>
                <span className="text-fg-muted">
                  {radarName(t.trigger)}
                  {t.reasons.length > 0 ? ` · ${t.reasons.map(radarName).join(', ')}` : ''}
                </span>
              </li>
            ))}
          </ol>
        )}
      </Section>

      <Section title="Evidence">
        {evidence.error && !evidence.data ? (
          <LoadFailure what="Evidence" failure={evidence.error} onRetry={evidence.retry} />
        ) : !evidence.data ? (
          <Skeleton className="h-16 w-full" />
        ) : evidence.data.items.length === 0 ? (
          <span className="text-xs text-fg-muted">No evidence recorded.</span>
        ) : (
          <ul
            aria-label="Verified swaps that are this wallet's evidence"
            className="m-0 flex list-none flex-col gap-1 p-0 text-xs"
          >
            {evidence.data.items.map((x) => {
              const f = finalityLabel(x.finality);
              return (
                <li key={x.swap_id} className="flex flex-col">
                  <span className="flex flex-wrap items-center gap-x-2">
                    {x.block_time ? (
                      <Timestamp iso={x.block_time} className="text-fg-secondary" />
                    ) : (
                      <span className="num text-fg-secondary">slot {n(x.slot)}</span>
                    )}
                    <span className="text-fg">{protocolName(x.protocol)}</span>
                    <StatusIndicator tone={f.tone} label={f.label} />
                    {x.valid ? null : (
                      <StatusIndicator
                        tone="warning"
                        label="Invalidated"
                        detail={
                          x.invalidation_reason ? radarName(x.invalidation_reason) : undefined
                        }
                      />
                    )}
                  </span>
                  <span className="text-fg-muted">
                    <Id value={x.signature} keep={6} /> · {radarName(x.economic_role)} ·{' '}
                    {radarName(x.discovery_source)}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </Section>
    </div>
  );
}

export function WalletInspector({
  address,
  onClose,
}: {
  address: string | null;
  onClose: () => void;
}) {
  const { data, loading, error, retry } = useApi(useRadarWallet(address));
  const notFound = error?.kind === 'http' && error.status === 404;
  return (
    <Drawer
      open={address !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title="Wallet"
      description="RADAR registry entry. Read-only; investigation state, not a recommendation."
    >
      {notFound ? (
        <DataState kind="empty" title="This wallet is not in the registry." />
      ) : error && !data ? (
        <LoadFailure what="Wallet detail" failure={error} onRetry={retry} />
      ) : loading || !data || !address ? (
        <div className="flex flex-col gap-2" aria-busy="true">
          {[0, 1, 2, 3, 4, 5, 6].map((i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : (
        <Detail d={data} address={address} />
      )}
    </Drawer>
  );
}
