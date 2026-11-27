/**
 * System-state panes. Every value comes from the real backend; nothing is
 * estimated, cached from earlier diagnostics or invented to fill space.
 */
import type { ReactNode } from 'react';

import { useConnection } from '@/components/shell/useConnection.ts';
import { Button } from '@/components/ui/Button.tsx';
import { DataState, Skeleton } from '@/components/ui/DataState.tsx';
import { DataAge, MetricCell, Timestamp } from '@/components/ui/Metric.tsx';
import { healthLabel } from '@/features/gateway/labels.ts';
import { DefinitionList, Panel } from '@/components/ui/Panel.tsx';
import { StatusIndicator } from '@/components/ui/StatusIndicator.tsx';
import { TBody, THead, Table, Td, Th, Tr } from '@/components/ui/Table.tsx';
import { describeFailure } from '@/lib/api/client.ts';
import { STALE_AFTER_MS, useSystemStatus, useVersion } from '@/lib/api/queries.ts';
import { formatInteger, formatLatencyUs } from '@/lib/format/number.ts';
import { formatDuration, formatUtcClock } from '@/lib/format/time.ts';
import type {
  CredentialState,
  ExecutionCapability as ExecutionCapabilityValue,
  ProviderStatus,
  SystemStatus,
} from '@/types/api.ts';

function useStatus() {
  const q = useSystemStatus();
  return {
    status: q.data?.data,
    loading: q.isPending && !q.error,
    error: q.error ? describeFailure(q.error) : null,
    updatedAt: q.dataUpdatedAt,
    retry: () => void q.refetch(),
  };
}

function Unreachable({ onRetry, what }: { onRetry: () => void; what: string }) {
  return (
    <DataState
      kind="error"
      title={`${what} unavailable: backend unreachable.`}
      detail="No values are shown until the backend answers. Retrying is safe (read-only)."
      action={
        <Button size="sm" onClick={onRetry}>
          Retry
        </Button>
      }
    />
  );
}

function StatusMeta() {
  const { status, updatedAt } = useStatus();
  return status ? <DataAge at={updatedAt} staleAfterMs={STALE_AFTER_MS} /> : null;
}

/** Safety first: what the system is allowed to do right now. */
export function OperatingStatePanel({ className }: { className?: string }) {
  const { status, loading, error, retry } = useStatus();
  return (
    <Panel title="Operating state" meta={<StatusMeta />} className={className}>
      {error && !status ? (
        <Unreachable what="Operating state" onRetry={retry} />
      ) : (
        <div className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4">
          <MetricCell
            size="lg"
            label="Trading mode"
            loading={loading}
            value={status ? status.trading.mode : null}
          />
          <MetricCell
            label="Live trading"
            loading={loading}
            value={
              status ? (
                status.trading.live_trading_enabled ? (
                  <StatusIndicator tone="negative" label="Enabled" />
                ) : (
                  <StatusIndicator tone="neutral" label="Disabled" />
                )
              ) : null
            }
          />
          <MetricCell
            label="Execution capability"
            loading={loading}
            value={status ? <ExecutionCapability status={status} /> : null}
          />
          <MetricCell label="Environment" loading={loading} value={status?.environment} />
        </div>
      )}
    </Panel>
  );
}

const executionLabel: Record<ExecutionCapabilityValue, string> = {
  NONE: 'None in this build',
};

function ExecutionCapability({ status }: { status: SystemStatus }) {
  return (
    <span className="text-sm text-fg-secondary">
      {executionLabel[status.trading.execution_capability]}
    </span>
  );
}

/** Backend, database and migration health from readiness + status. */
export function HealthPanel({ className }: { className?: string }) {
  const { status, loading, error, retry } = useStatus();
  const { state, dbLatencyUs } = useConnection();
  const db = status?.database;
  return (
    <Panel title="System health" meta={<StatusMeta />} className={className}>
      {error && !status ? (
        <Unreachable what="Health" onRetry={retry} />
      ) : (
        <div className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-3">
          <MetricCell
            label="Backend"
            loading={state.kind === 'connecting'}
            value={<BackendState kind={state.kind} />}
          />
          <MetricCell
            label="Database"
            loading={loading}
            value={
              db ? (
                db.status === 'AVAILABLE' ? (
                  <StatusIndicator tone="positive" label="Available" />
                ) : (
                  <StatusIndicator tone="negative" label="Unavailable" />
                )
              ) : null
            }
          />
          <MetricCell
            label="DB probe latency"
            loading={loading}
            {...(db && db.status === 'UNAVAILABLE'
              ? { unavailable: 'no probe' }
              : { value: formatLatencyUs(dbLatencyUs ?? db?.latency_us) })}
          />
          <MetricCell
            label="Migrations"
            loading={loading}
            {...(db && db.migrations_applied === null
              ? { unavailable: 'unknown' }
              : {
                  value: db
                    ? `${formatInteger(db.migrations_applied ?? 0)} / ${formatInteger(db.migrations_expected)}`
                    : null,
                })}
          />
          <MetricCell
            label="Uptime"
            loading={loading}
            value={status ? formatDuration(status.uptime_seconds) : null}
          />
          <MetricCell
            label="Started"
            loading={loading}
            value={
              status ? (
                <span className="text-sm">{formatUtcClock(Date.parse(status.started_at))} UTC</span>
              ) : null
            }
          />
        </div>
      )}
    </Panel>
  );
}

function BackendState({ kind }: { kind: ReturnType<typeof useConnection>['state']['kind'] }) {
  switch (kind) {
    case 'connected':
      return <StatusIndicator tone="positive" label="Ready" />;
    case 'degraded':
      return <StatusIndicator tone="warning" label="Degraded" />;
    case 'stale':
      return <StatusIndicator tone="warning" label="Stale" />;
    case 'offline':
      return <StatusIndicator tone="negative" label="Offline" />;
    case 'connecting':
      return <StatusIndicator tone="neutral" label="Connecting" />;
  }
}

const credentialLabel: Record<CredentialState, { tone: 'neutral' | 'positive'; label: string }> = {
  CONFIGURED: { tone: 'positive', label: 'Configured' },
  ABSENT: { tone: 'neutral', label: 'Not set' },
  NOT_REQUIRED: { tone: 'neutral', label: 'Not required' },
};

/** External providers: configuration plus GATEWAY's measured health. */
export function ProvidersPanel({
  className,
  showRole = true,
}: {
  className?: string | undefined;
  showRole?: boolean;
}) {
  const { status, loading, error, retry } = useStatus();
  return (
    <Panel
      title="Providers"
      flush
      className={className}
      meta={
        status ? <span>{formatInteger(status.providers.length)} configured endpoints</span> : null
      }
    >
      {error && !status ? (
        <div className="p-3">
          <Unreachable what="Provider state" onRetry={retry} />
        </div>
      ) : (
        <>
          <Table label="Provider integration state">
            <THead>
              <Tr>
                <Th>Provider</Th>
                {showRole ? <Th className="hidden lg:table-cell">Role</Th> : null}
                <Th className="hidden sm:table-cell">Credential</Th>
                <Th>State</Th>
                <Th numeric className="hidden sm:table-cell">
                  Last checked
                </Th>
              </Tr>
            </THead>
            <TBody>
              {loading
                ? [0, 1, 2, 3].map((i) => (
                    <Tr key={i}>
                      <Td colSpan={showRole ? 5 : 4}>
                        <Skeleton className="h-3 w-full" />
                      </Td>
                    </Tr>
                  ))
                : status?.providers.map((p) => (
                    <ProviderRow key={p.id} provider={p} showRole={showRole} />
                  ))}
            </TBody>
          </Table>
          <p className="m-0 border-t border-line-subtle px-3 py-2 text-xs text-fg-muted">
            Health is measured by GATEWAY from real requests only; no background probe runs. Earlier
            manual diagnostics are never shown as current measurements.
          </p>
        </>
      )}
    </Panel>
  );
}

function ProviderRow({ provider, showRole }: { provider: ProviderStatus; showRole: boolean }) {
  const cred = credentialLabel[provider.credential];
  return (
    <Tr>
      <Td>
        <span className="font-medium text-fg">{provider.name}</span>
        <span className="mt-0.5 block sm:hidden">
          <StatusIndicator tone={cred.tone} label={cred.label} />
        </span>
      </Td>
      {showRole ? (
        <Td className="hidden text-fg-secondary lg:table-cell">{provider.role}</Td>
      ) : null}
      <Td className="hidden sm:table-cell">
        <StatusIndicator tone={cred.tone} label={cred.label} />
      </Td>
      <Td>
        <StatusIndicator
          tone={healthLabel[provider.state].tone}
          label={healthLabel[provider.state].label}
        />
      </Td>
      <Td numeric className="hidden text-fg-muted sm:table-cell">
        {provider.last_checked_at ? <Timestamp iso={provider.last_checked_at} /> : 'Never'}
      </Td>
    </Tr>
  );
}

const MODULES: { name: string; responsibility: string }[] = [
  { name: 'GATEWAY', responsibility: 'Provider communication, caching, rate limits, API budgets' },
  { name: 'DATA ENGINE', responsibility: 'Ingestion, transaction decoding, event normalization' },
  { name: 'RADAR', responsibility: 'Wallet discovery, monitoring, ranking' },
  { name: 'FILTER', responsibility: 'Token security, liquidity and manipulation analysis' },
  { name: 'CORTEX', responsibility: 'Quantitative strategy and decisions' },
  { name: 'GUARD', responsibility: 'Independent risk management and authorization' },
  { name: 'REPLAY', responsibility: 'Historical backtesting' },
  { name: 'SANDBOX', responsibility: 'Real-time paper trading' },
  { name: 'SNIPER', responsibility: 'Authorized execution and reconciliation' },
  { name: 'TERMINAL', responsibility: 'Monitoring, analytics and control' },
];

/** Architecture status. Static by design: it states which modules exist. */
export function ModulesPanel({ className }: { className?: string }) {
  return (
    <Panel
      title="Modules"
      flush
      className={className}
      meta={<span>GATEWAY and DATA ENGINE implemented · other modules not yet</span>}
    >
      <Table label="SOLPRINT modules">
        <THead>
          <Tr>
            <Th>Module</Th>
            <Th className="hidden sm:table-cell">Responsibility</Th>
            <Th>State</Th>
          </Tr>
        </THead>
        <TBody>
          {MODULES.map((m) => (
            <Tr key={m.name}>
              <Td className="whitespace-normal">
                <span className="text-2xs font-semibold tracking-wider text-fg">{m.name}</span>
                <span className="block text-xs text-fg-muted sm:hidden">{m.responsibility}</span>
              </Td>
              <Td className="hidden text-fg-secondary sm:table-cell">{m.responsibility}</Td>
              <Td>
                {m.name === 'TERMINAL' ? (
                  <StatusIndicator tone="info" label="Foundation shell" />
                ) : m.name === 'GATEWAY' || m.name === 'DATA ENGINE' ? (
                  <StatusIndicator tone="info" label="Read-only" />
                ) : (
                  <StatusIndicator tone="neutral" label="Not implemented" />
                )}
              </Td>
            </Tr>
          ))}
        </TBody>
      </Table>
    </Panel>
  );
}

export function RuntimePanel({ className }: { className?: string }) {
  const { status, loading, error, retry } = useStatus();
  return (
    <Panel title="Runtime" meta={<StatusMeta />} className={className}>
      {error && !status ? (
        <Unreachable what="Runtime information" onRetry={retry} />
      ) : (
        <PanelRows
          loading={loading}
          rows={
            status
              ? [
                  { term: 'Application', value: status.application },
                  { term: 'Environment', value: status.environment },
                  { term: 'Started', value: <Timestamp iso={status.started_at} /> },
                  {
                    term: 'Uptime',
                    value: <span className="num">{formatDuration(status.uptime_seconds)}</span>,
                  },
                  { term: 'Server time', value: <Timestamp iso={status.timestamp} /> },
                ]
              : []
          }
        />
      )}
    </Panel>
  );
}

export function DatabasePanel({ className }: { className?: string }) {
  const { status, loading, error, retry } = useStatus();
  const db = status?.database;
  return (
    <Panel title="Database" meta={<StatusMeta />} className={className}>
      {error && !status ? (
        <Unreachable what="Database state" onRetry={retry} />
      ) : (
        <PanelRows
          loading={loading}
          rows={
            db
              ? [
                  {
                    term: 'PostgreSQL',
                    value:
                      db.status === 'AVAILABLE' ? (
                        <StatusIndicator tone="positive" label="Available" />
                      ) : (
                        <StatusIndicator tone="negative" label="Unavailable" />
                      ),
                  },
                  {
                    term: 'Probe latency',
                    value: <span className="num">{formatLatencyUs(db.latency_us)}</span>,
                  },
                  {
                    term: 'Migrations',
                    value:
                      db.migrations_applied === null ? (
                        <span className="text-fg-muted">Unknown (database unavailable)</span>
                      ) : (
                        <span className="num">
                          {formatInteger(db.migrations_applied)} applied ·{' '}
                          {formatInteger(db.migrations_expected)} in build
                        </span>
                      ),
                  },
                ]
              : []
          }
        />
      )}
    </Panel>
  );
}

export function BuildPanel({ className }: { className?: string }) {
  const q = useVersion();
  const v = q.data?.data;
  return (
    <Panel title="Build" className={className}>
      {q.error && !v ? (
        <Unreachable what="Build information" onRetry={() => void q.refetch()} />
      ) : (
        <PanelRows
          loading={q.isPending}
          rows={
            v
              ? [
                  { term: 'Version', value: <span className="num">{v.version}</span> },
                  { term: 'API', value: v.api_version },
                  {
                    term: 'Commit',
                    value: v.git_commit ? (
                      <span className="font-mono text-xs">{v.git_commit.slice(0, 12)}</span>
                    ) : (
                      <span className="text-fg-muted">Not embedded</span>
                    ),
                  },
                  { term: 'Profile', value: v.build_profile },
                ]
              : []
          }
        />
      )}
    </Panel>
  );
}

export function ReadinessPanel({ className }: { className?: string }) {
  const { state } = useConnection();
  const detail: Record<typeof state.kind, ReactNode> = {
    connecting: 'Waiting for the first readiness report.',
    connected: 'All critical dependencies answered. The backend is ready to serve.',
    degraded: 'The backend is running but a critical dependency is unavailable.',
    stale: 'Readiness reports have stopped arriving. Values on this page may be outdated.',
    offline: 'The backend cannot be reached. Nothing on this page is current.',
  };
  return (
    <Panel title="Readiness" className={className}>
      <div className="flex flex-col gap-2">
        <BackendState kind={state.kind} />
        <p className="m-0 text-sm text-fg-secondary">{detail[state.kind]}</p>
      </div>
    </Panel>
  );
}

function PanelRows({
  loading,
  rows,
}: {
  loading: boolean;
  rows: { term: string; value: ReactNode }[];
}) {
  if (loading) {
    return (
      <div className="flex flex-col gap-2" aria-busy="true">
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="h-5 w-full" />
        ))}
      </div>
    );
  }
  return <DefinitionList items={rows} />;
}
