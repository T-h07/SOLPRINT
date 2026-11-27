/**
 * Transaction inspector: evidence, balance changes, activities, swaps and
 * issues of one decoded transaction, in a side drawer that keeps the list
 * context. Read-only; nothing here can queue or change work.
 */
import type { ReactNode } from 'react';

import { DataState, Skeleton } from '@/components/ui/DataState.tsx';
import { Drawer } from '@/components/ui/Drawer.tsx';
import { Timestamp } from '@/components/ui/Metric.tsx';
import { DefinitionList } from '@/components/ui/Panel.tsx';
import { StatusIndicator } from '@/components/ui/StatusIndicator.tsx';
import { TBody, THead, Table, Td, Th, Tr } from '@/components/ui/Table.tsx';
import { useDataTransaction } from '@/lib/api/queries.ts';
import {
  formatBytes,
  formatDecimalString,
  formatInteger,
  formatLamportsAsSol,
  formatSignedBaseUnits,
  formatTokenAmount,
  shortId,
} from '@/lib/format/number.ts';
import type { DataTransactionDetail, SwapDto } from '@/types/api.ts';

import { LoadFailure } from '../shared/pane.tsx';
import { useApi } from '../shared/useApi.ts';
import {
  attributionLabel,
  decodeLabel,
  finalityLabel,
  humanize,
  protocolName,
  severityLabel,
} from './labels.ts';
import { Amount, Id } from './panels.tsx';

const n = (v: number) => formatInteger(v);

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2 border-t border-line pt-3">
      <h3 className="text-sm font-medium text-fg">{title}</h3>
      {children}
    </section>
  );
}

function SwapBlock({ s }: { s: SwapDto }) {
  const a = attributionLabel(s.attribution);
  const st = decodeLabel(s.classification_status);
  return (
    <div className="flex flex-col gap-2 rounded-sm border border-line-subtle p-2">
      <DefinitionList
        items={[
          {
            term: 'Venue',
            wrap: true,
            value: (
              <span>
                {protocolName(s.protocol)}{' '}
                <span className="text-fg-muted">· {humanize(s.venue_kind)}</span>
                <span className="num text-fg-muted"> · ix {s.instruction_ref}</span>
              </span>
            ),
          },
          { term: 'Status', value: <StatusIndicator tone={st.tone} label={st.label} /> },
          ...(s.canonical
            ? []
            : [
                {
                  term: 'Retracted',
                  wrap: true,
                  value: (
                    <span className="text-fg-secondary">
                      {humanize(s.retraction_reason ?? 'revision')}
                    </span>
                  ),
                },
              ]),
          {
            term: 'Attribution',
            wrap: true,
            value: (
              <span className="inline-flex flex-wrap items-center gap-2">
                <StatusIndicator tone={a.tone} label={a.label} />
                {s.wallet ? (
                  <Id value={s.wallet} keep={6} />
                ) : (
                  <span className="text-fg-muted">no wallet asserted</span>
                )}
              </span>
            ),
          },
          { term: 'Sent', value: <Amount asset={s.input} /> },
          { term: 'Received', value: <Amount asset={s.output} /> },
          {
            term: 'Out per in',
            value: (
              <span className="num">
                {s.exchange_ratio ? formatDecimalString(s.exchange_ratio, 9) : '—'}
              </span>
            ),
          },
          {
            term: 'Route',
            value: s.route_complete ? (
              <span className="text-fg-secondary">Complete</span>
            ) : (
              <StatusIndicator tone="warning" label="Incomplete" />
            ),
          },
          {
            term: 'USD value',
            wrap: true,
            value: (
              <span className="text-fg-muted">
                {s.valuation.state === 'NOT_ENRICHED'
                  ? 'Not enriched (price enrichment off)'
                  : humanize(s.valuation.state)}
              </span>
            ),
          },
          ...(s.reasons.length > 0
            ? [
                {
                  term: 'Reasons',
                  wrap: true,
                  value: (
                    <span className="font-mono text-xs text-fg-secondary">
                      {s.reasons.join(' · ')}
                    </span>
                  ),
                },
              ]
            : []),
        ]}
      />
      {s.legs.length > 0 ? (
        <Table label="Route legs">
          <THead>
            <Tr>
              <Th numeric>#</Th>
              <Th>Venue</Th>
              <Th numeric>In (raw)</Th>
              <Th numeric>Out (raw)</Th>
            </Tr>
          </THead>
          <TBody>
            {s.legs.map((l) => (
              <Tr key={l.index}>
                <Td numeric>{n(l.index + 1)}</Td>
                <Td>
                  {protocolName(l.protocol)}
                  <span className="block text-2xs text-fg-muted">{humanize(l.evidence)}</span>
                </Td>
                <Td numeric>{formatTokenAmount(l.input_amount, 0)}</Td>
                <Td numeric>{formatTokenAmount(l.output_amount, 0)}</Td>
              </Tr>
            ))}
          </TBody>
        </Table>
      ) : null}
      {s.fees.length > 0 ? (
        <ul aria-label="Swap fees" className="m-0 flex list-none flex-col gap-0.5 p-0 text-xs">
          {s.fees.map((f, i) => (
            <li key={`${f.kind}-${i.toString()}`} className="num text-fg-secondary">
              {humanize(f.kind)} fee {formatTokenAmount(f.amount, 0)} raw{' '}
              <span className="text-fg-muted">· {humanize(f.provenance)}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function Detail({ d }: { d: DataTransactionDetail }) {
  const t = d.summary;
  const dec = decodeLabel(t.decode_status);
  const fin = finalityLabel(t.finality);
  return (
    <div className="flex flex-col gap-3">
      <DefinitionList
        items={[
          {
            term: 'Signature',
            wrap: true,
            value: <span className="font-mono text-xs break-all">{t.signature}</span>,
          },
          {
            term: 'Slot',
            wrap: true,
            value: (
              <span className="num">
                {n(t.slot)}
                {t.block_time ? (
                  <span className="text-fg-muted">
                    {' '}
                    · <Timestamp iso={t.block_time} />
                  </span>
                ) : null}
              </span>
            ),
          },
          {
            term: 'Execution',
            value:
              t.execution_status === 'FAILED' ? (
                <StatusIndicator tone="negative" label="Failed on chain" />
              ) : (
                <span className="text-fg-secondary">Succeeded</span>
              ),
          },
          { term: 'Decode', value: <StatusIndicator tone={dec.tone} label={dec.label} /> },
          {
            term: 'Finality',
            value: (
              <StatusIndicator
                tone={fin.tone}
                label={fin.label}
                detail={`${n(d.finality_checks)} checks`}
              />
            ),
          },
          {
            term: 'Fee',
            value: <span className="num">{formatLamportsAsSol(t.fee_lamports, 9)} SOL</span>,
          },
          { term: 'Fee payer', value: <Id value={t.fee_payer} keep={6} /> },
          { term: 'Signers', value: <span className="num">{n(d.signers.length)}</span> },
          {
            term: 'Decoder',
            value: (
              <span className="font-mono text-xs">
                {t.decoder_version} · r{n(t.revision)}
              </span>
            ),
          },
        ]}
      />

      <Section title={`Swaps (${n(d.swaps.length)})`}>
        {d.swaps.length === 0 ? (
          <p className="m-0 text-xs text-fg-muted">No user-level swap in this transaction.</p>
        ) : (
          d.swaps.map((s) => <SwapBlock key={s.swap_id} s={s} />)
        )}
      </Section>

      <Section title={`Activities (${n(d.activities.length)})`}>
        <Table label="Activities in execution order">
          <THead>
            <Tr>
              <Th numeric>#</Th>
              <Th>Kind</Th>
              <Th>Status</Th>
              <Th numeric>Ix</Th>
            </Tr>
          </THead>
          <TBody>
            {d.activities.map((a) => {
              const st = decodeLabel(a.status);
              return (
                <Tr key={a.activity_id}>
                  <Td numeric>{n(a.index)}</Td>
                  <Td>
                    {humanize(a.kind)}
                    {a.protocol ? (
                      <span className="block text-2xs text-fg-muted">
                        {protocolName(a.protocol)}
                      </span>
                    ) : null}
                  </Td>
                  <Td>
                    <StatusIndicator tone={st.tone} label={st.label} />
                  </Td>
                  <Td numeric>{a.instruction_ref ?? '—'}</Td>
                </Tr>
              );
            })}
          </TBody>
        </Table>
      </Section>

      <Section title={`Balance changes (${n(d.balances.length)})`}>
        <Table label="Pre and post balance deltas">
          <THead>
            <Tr>
              <Th>Account</Th>
              <Th>Asset</Th>
              <Th numeric>Delta</Th>
            </Tr>
          </THead>
          <TBody>
            {d.balances.map((b) => (
              <Tr key={`${b.asset}:${b.account}`}>
                <Td>
                  <Id value={b.account} />
                  {b.pre_evidence !== 'REPORTED' || b.post_evidence !== 'REPORTED' ? (
                    <span className="block text-2xs text-fg-muted">
                      {humanize(b.pre_evidence)} → {humanize(b.post_evidence)}
                    </span>
                  ) : null}
                </Td>
                <Td>{b.asset === 'SOL' ? 'SOL' : <Id value={b.asset} />}</Td>
                <Td numeric>{formatSignedBaseUnits(b.delta, b.decimals, 9)}</Td>
              </Tr>
            ))}
          </TBody>
        </Table>
      </Section>

      <Section title="Evidence">
        <ul
          aria-label="Evidence revisions"
          className="m-0 flex list-none flex-col gap-1 p-0 text-xs"
        >
          {d.evidence.map((e) => (
            <li key={e.revision} className="num text-fg-secondary">
              r{n(e.revision)} · {e.commitment} · {e.provider.toLowerCase()} ·{' '}
              {formatBytes(e.byte_len)} ·{' '}
              <span className="font-mono" title={e.content_sha256}>
                sha256 {shortId(e.content_sha256, 6)}
              </span>
              {e.canonical ? null : (
                <span className="text-fg-muted">
                  {' '}
                  · superseded ({humanize(e.superseded_reason ?? 'revision')})
                </span>
              )}
              {e.pruned ? <span className="text-fg-muted"> · raw evidence pruned</span> : null}
            </li>
          ))}
        </ul>
      </Section>

      {d.issues.length > 0 ? (
        <Section title={`Issues (${n(d.issues.length)})`}>
          <ul aria-label="Quality issues" className="m-0 flex list-none flex-col gap-1 p-0 text-xs">
            {d.issues.map((i) => {
              const l = severityLabel(i.severity);
              return (
                <li key={i.issue_id} className="flex flex-wrap items-center gap-2">
                  <StatusIndicator tone={l.tone} label={l.label} />
                  <span className="font-mono">{i.code}</span>
                  {i.detail ? <span className="text-fg-muted">{i.detail}</span> : null}
                </li>
              );
            })}
          </ul>
        </Section>
      ) : null}
    </div>
  );
}

export function TransactionInspector({
  signature,
  onClose,
}: {
  signature: string | null;
  onClose: () => void;
}) {
  const { data, loading, error, retry } = useApi(useDataTransaction(signature));
  const notFound = error?.kind === 'http' && error.status === 404;
  return (
    <Drawer
      open={signature !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title="Transaction"
      description="Decoded from stored evidence. Read-only."
    >
      {notFound ? (
        <DataState kind="empty" title="This transaction is not stored." />
      ) : error && !data ? (
        <LoadFailure what="Transaction detail" failure={error} onRetry={retry} />
      ) : loading || !data ? (
        <div className="flex flex-col gap-2" aria-busy="true">
          {[0, 1, 2, 3, 4, 5, 6].map((i) => (
            <Skeleton key={i} className="h-5 w-full" />
          ))}
        </div>
      ) : (
        <Detail d={data} />
      )}
    </Drawer>
  );
}
