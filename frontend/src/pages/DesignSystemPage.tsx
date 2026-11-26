import { Copy, OctagonX, RefreshCw } from 'lucide-react';
import { type ReactNode, useState } from 'react';

import { Button, IconButton } from '@/components/ui/Button.tsx';
import { DataState, FixtureFrame, Skeleton } from '@/components/ui/DataState.tsx';
import { Drawer } from '@/components/ui/Drawer.tsx';
import { Field, Input, Select } from '@/components/ui/Field.tsx';
import { MetricCell } from '@/components/ui/Metric.tsx';
import { DefinitionList, Panel } from '@/components/ui/Panel.tsx';
import { StatusIndicator, type Tone } from '@/components/ui/StatusIndicator.tsx';
import { type Density, TBody, THead, Table, Td, Th, Tr } from '@/components/ui/Table.tsx';
import { fixtureTokenAmounts, fixtureWallets } from '@/fixtures/designSystem.ts';
import { PageLayout } from '@/layouts/PageLayout.tsx';
import { cn } from '@/lib/cn.ts';
import {
  formatBaseUnits,
  formatBpsAsPercent,
  formatInteger,
  formatLamportsAsSol,
  formatLatencyUs,
  MINUS,
} from '@/lib/format/number.ts';
import { formatDuration } from '@/lib/format/time.ts';

/**
 * Internal component showcase and visual-regression reference. Development
 * builds only (or VITE_ENABLE_DESIGN_SYSTEM=true). Everything below that
 * looks like trading data is fixture data and is framed as such.
 */
export default function DesignSystemPage() {
  return (
    <PageLayout title="Design system" context="Internal · development builds only">
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        <TokensPanel />
        <TypographyPanel />
        <ButtonsPanel />
        <FormsPanel />
        <StatusPanel />
        <DataStatesPanel />
        <MetricsPanel />
        <SafetyPanel />
        <TablePanel />
        <ChartTokensPanel />
        <NumberFormattingPanel />
        <GeometryPanel />
      </div>
    </PageLayout>
  );
}

function Swatch({
  name,
  value,
  cls,
  note,
}: {
  name: string;
  value: string;
  cls: string;
  note?: string;
}) {
  return (
    <div className="flex items-center gap-3">
      <span
        className={cn('size-6 shrink-0 rounded-xs border border-line', cls)}
        aria-hidden="true"
      />
      <span className="min-w-0 flex-1">
        <span className="block font-mono text-2xs text-fg">{name}</span>
        <span className="num block text-2xs text-fg-muted">
          {value}
          {note ? ` · ${note}` : ''}
        </span>
      </span>
    </div>
  );
}

function TokensPanel() {
  const surfaces: [string, string, string][] = [
    ['canvas', '#050505', 'bg-canvas'],
    ['base', '#080808', 'bg-base'],
    ['raised', '#0D0D0D', 'bg-raised'],
    ['hover', '#141414', 'bg-hover'],
    ['line-subtle', '#1A1A1A', 'bg-line-subtle'],
    ['line', '#252525', 'bg-line'],
    ['line-control', '#666666', 'bg-line-control'],
  ];
  const text: [string, string, string, string][] = [
    ['fg', '#F2F2F2', 'bg-fg', '16.5–18.2:1'],
    ['fg-secondary', '#A3A3A3', 'bg-fg-secondary', '7.3–8.1:1'],
    ['fg-muted', '#808080', 'bg-fg-muted', '4.7–5.2:1'],
    ['fg-disabled', '#525252', 'bg-fg-disabled', 'disabled only'],
  ];
  const semantic: [string, string, string][] = [
    ['positive', '#2FBF71', 'bg-positive'],
    ['negative', '#F2555A', 'bg-negative'],
    ['warning', '#E8A33D', 'bg-warning'],
    ['info', '#6CA6E8', 'bg-info'],
    ['live', '#B4232A', 'bg-live'],
    ['focus', '#70B8FF', 'bg-focus'],
  ];
  return (
    <Panel title="Colour tokens">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <div className="flex flex-col gap-2">
          <h3 className="text-2xs tracking-wider text-fg-muted uppercase">Surfaces & lines</h3>
          {surfaces.map(([n, v, c]) => (
            <Swatch key={n} name={n} value={v} cls={c} />
          ))}
        </div>
        <div className="flex flex-col gap-2">
          <h3 className="text-2xs tracking-wider text-fg-muted uppercase">Foreground</h3>
          {text.map(([n, v, c, note]) => (
            <Swatch key={n} name={n} value={v} cls={c} note={note} />
          ))}
        </div>
        <div className="flex flex-col gap-2">
          <h3 className="text-2xs tracking-wider text-fg-muted uppercase">Semantic</h3>
          {semantic.map(([n, v, c]) => (
            <Swatch key={n} name={n} value={v} cls={c} />
          ))}
        </div>
      </div>
    </Panel>
  );
}

function TypographyPanel() {
  const rows: [string, string, ReactNode][] = [
    ['text-lg · 20/28', 'Page title', <span className="text-lg font-medium">System</span>],
    [
      'text-md · 15/20',
      'Section title',
      <span className="text-md font-medium">Operating state</span>,
    ],
    [
      'text-xl · 24/32',
      'Primary metric',
      <span className="num text-xl font-medium">1,284.5021</span>,
    ],
    [
      'text-sm · 13/20',
      'Interface text',
      <span className="text-sm">Backend ready · database available</span>,
    ],
    [
      'text-xs · 12/16',
      'Dense table data',
      <span className="num text-xs">0.000012345 · 214 · 61.20%</span>,
    ],
    [
      'text-2xs · 11/16',
      'Auxiliary label',
      <span className="text-2xs tracking-wider text-fg-muted uppercase">Last checked</span>,
    ],
    [
      'font-mono',
      'Identifiers',
      <span className="font-mono text-xs">FX-W-0001 · 0198c4e2-7a1f</span>,
    ],
  ];
  return (
    <Panel title="Typography">
      <div className="flex flex-col divide-y divide-line-subtle">
        {rows.map(([spec, role, sample]) => (
          <div key={spec} className="grid grid-cols-[9rem_7rem_1fr] items-baseline gap-3 py-2">
            <span className="font-mono text-2xs text-fg-muted">{spec}</span>
            <span className="text-xs text-fg-secondary">{role}</span>
            <span className="min-w-0 truncate">{sample}</span>
          </div>
        ))}
      </div>
    </Panel>
  );
}

function ButtonsPanel() {
  const [pressed, setPressed] = useState(true);
  return (
    <Panel title="Buttons">
      <div className="flex flex-col gap-3">
        <Row label="Variants">
          <Button variant="primary">Primary</Button>
          <Button>Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="destructive">Destructive</Button>
        </Row>
        <Row label="Sizes">
          <Button size="sm">Small 28</Button>
          <Button size="md">Medium 32</Button>
          <Button size="lg">Large 40</Button>
        </Row>
        <Row label="Selected">
          <Button
            aria-pressed={pressed}
            onClick={() => {
              setPressed((p) => !p);
            }}
          >
            Compact density
          </Button>
          <Button variant="ghost" aria-pressed>
            Ghost selected
          </Button>
        </Row>
        <Row label="Disabled">
          <Button variant="primary" disabled>
            Primary
          </Button>
          <Button disabled>Secondary</Button>
          <Button variant="destructive" disabled>
            Destructive
          </Button>
        </Row>
        <Row label="Icon">
          <IconButton label="Refresh" icon={<RefreshCw className="size-4" />} />
          <IconButton
            label="Copy identifier"
            variant="secondary"
            icon={<Copy className="size-4" />}
          />
          <IconButton label="Refresh (disabled)" disabled icon={<RefreshCw className="size-4" />} />
        </Row>
        <p className="m-0 text-xs text-fg-muted">
          Hover, pressed and focus-visible states are interactive here; Tab through to inspect the
          focus ring, which is distinct from the selected treatment.
        </p>
      </div>
    </Panel>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="w-20 shrink-0 text-2xs tracking-wider text-fg-muted uppercase">{label}</span>
      {children}
    </div>
  );
}

function FormsPanel() {
  return (
    <Panel title="Inputs">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <Field label="Filter" hint="Matches label or identifier.">
          {(p) => <Input placeholder="fixture-alpha" {...p} />}
        </Field>
        <Field label="Max slippage (bps)" error="Must be between 0 and 10000.">
          {(p) => <Input className="num" defaultValue="12000" inputMode="numeric" {...p} />}
        </Field>
        <Field label="Density">
          {(p) => (
            <Select defaultValue="compact" {...p}>
              <option value="compact">Compact · 32 px</option>
              <option value="standard">Standard · 40 px</option>
            </Select>
          )}
        </Field>
        <Field label="Disabled" hint="Read-only in Phase 00.">
          {(p) => <Input disabled defaultValue="paper" {...p} />}
        </Field>
      </div>
    </Panel>
  );
}

function StatusPanel() {
  const tones: [Tone, string, string][] = [
    ['positive', 'Healthy', 'filled circle'],
    ['warning', 'Degraded', 'triangle'],
    ['negative', 'Failed', 'square'],
    ['info', 'Selected series', 'diamond'],
    ['neutral', 'Not initialized', 'hollow circle'],
  ];
  return (
    <Panel title="Status indicators">
      <div className="flex flex-col gap-2">
        {tones.map(([tone, label, shape]) => (
          <div key={tone} className="flex items-center gap-4">
            <span className="w-32">
              <StatusIndicator tone={tone} label={label} />
            </span>
            <span className="text-xs text-fg-muted">
              {tone} · {shape}
            </span>
          </div>
        ))}
        <p className="m-0 pt-1 text-xs text-fg-muted">
          Shape and label carry the meaning; colour reinforces it. Readable in greyscale.
        </p>
      </div>
    </Panel>
  );
}

function DataStatesPanel() {
  return (
    <Panel title="Data states">
      <div className="flex flex-col divide-y divide-line-subtle">
        <DataState
          kind="empty"
          title="No qualified wallets yet."
          detail="Radar last scanned 38s ago. (fixture copy)"
        />
        <DataState
          kind="error"
          title="Price data unavailable: Jupiter returned 503."
          detail="Retrying is safe (read-only). Last success 4m 10s ago. (fixture copy)"
          action={<Button size="sm">Retry</Button>}
        />
        <DataState
          kind="stale"
          title="Showing last known values."
          detail="STALE · 2m 14s — no readiness report since 14:01:08 UTC. (fixture copy)"
        />
        <DataState
          kind="partial"
          title="2 of 4 providers reporting."
          detail="Missing: DEX Screener, Solana RPC. (fixture copy)"
        />
        <DataState
          kind="not-initialized"
          title="Provider client not initialized."
          detail="No measurement has ever been taken."
        />
        <div className="flex flex-col gap-2 py-2" aria-busy="true">
          <span className="text-2xs tracking-wider text-fg-muted uppercase">
            Loading · geometry-matched
          </span>
          <Skeleton className="h-8 w-full" />
          <Skeleton className="h-8 w-full" />
        </div>
      </div>
    </Panel>
  );
}

function MetricsPanel() {
  return (
    <Panel title="Metric cells">
      <FixtureFrame>
        <div className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4">
          <MetricCell size="lg" label="Equity" value="1,284.5021" unit="SOL" />
          <MetricCell
            label="Unrealized P&L"
            value={<span className="text-positive">+12.0410</span>}
            unit="SOL"
            delta={<span className="text-positive">+0.94%</span>}
          />
          <MetricCell
            label="Drawdown"
            value={<span className="text-negative">{MINUS}3.2200</span>}
            unit="SOL"
            delta={<span className="text-negative">{MINUS}0.25%</span>}
          />
          <MetricCell label="Exposure" loading />
          <MetricCell label="Available capital" unavailable="not initialized" />
          <MetricCell label="RPC latency" value={formatLatencyUs(18_400)} />
        </div>
      </FixtureFrame>
    </Panel>
  );
}

function SafetyPanel() {
  return (
    <Panel title="Safety controls">
      <FixtureFrame label="Fixture · visual specification only — not functional">
        <div className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center gap-3">
            <span className="inline-flex h-6 items-center rounded-sm border border-paper px-2 text-xs font-semibold tracking-wider text-paper uppercase">
              Paper mode
            </span>
            <StatusIndicator tone="neutral" label="Live trading disabled" />
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <span className="inline-flex h-6 items-center rounded-sm bg-live px-2 text-xs font-semibold tracking-wider text-live-fg uppercase">
              Live trading
            </span>
            <span className="font-mono text-xs text-fg-secondary">wallet FX-LIVE-WALLET</span>
            <StatusIndicator tone="positive" label="Guard armed" />
            <span className="num text-xs text-fg-muted">enabled 2026-11-26 09:00:00 UTC</span>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <Button variant="destructive" size="lg" disabled aria-describedby="estop-note">
              <OctagonX className="size-4" aria-hidden="true" />
              Emergency stop
            </Button>
            <span id="estop-note" className="text-xs text-fg-muted">
              Stable (non-flashing), labelled, keyboard reachable. Disabled here: no execution
              exists in Phase 00.
            </span>
          </div>
        </div>
      </FixtureFrame>
    </Panel>
  );
}

const walletStateTone: Record<string, Tone> = {
  TRACKED: 'positive',
  PAUSED: 'warning',
  REJECTED: 'negative',
};

function TablePanel() {
  const [density, setDensity] = useState<Density>('compact');
  const [selected, setSelected] = useState<string | null>(null);
  const [inspect, setInspect] = useState<string | null>(null);
  const wallet = fixtureWallets.find((w) => w.id === inspect);
  return (
    <Panel
      title="Data table"
      flush
      className="xl:col-span-2"
      actions={
        <div className="flex gap-1" role="group" aria-label="Row density">
          {(['compact', 'standard'] as const).map((d) => (
            <Button
              key={d}
              size="sm"
              variant="ghost"
              aria-pressed={density === d}
              onClick={() => {
                setDensity(d);
              }}
            >
              {d === 'compact' ? 'Compact' : 'Standard'}
            </Button>
          ))}
        </div>
      }
    >
      <div className="p-3">
        <FixtureFrame className="p-0 pt-4">
          <Table label="Fixture wallet table" density={density}>
            <THead>
              <Tr>
                <Th>Wallet</Th>
                <Th numeric>Score</Th>
                <Th numeric>Realized P&L (SOL)</Th>
                <Th numeric>Win rate</Th>
                <Th numeric>Max drawdown</Th>
                <Th numeric>Trades</Th>
                <Th numeric>Last active</Th>
                <Th>State</Th>
                <Th>
                  <span className="sr-only">Actions</span>
                </Th>
              </Tr>
            </THead>
            <TBody>
              {fixtureWallets.map((w) => (
                <Tr
                  key={w.id}
                  selected={selected === w.id}
                  onClick={() => {
                    setSelected(w.id);
                  }}
                >
                  <Td>
                    <span className="font-mono text-2xs text-fg-muted">{w.id}</span>{' '}
                    <span className="text-fg">{w.label}</span>
                  </Td>
                  <Td numeric>{formatInteger(w.score)}</Td>
                  <Td numeric className={w.pnlNegative ? 'text-negative' : 'text-positive'}>
                    {w.pnlNegative ? MINUS : '+'}
                    {formatLamportsAsSol(w.realizedPnlLamports, 4)}
                  </Td>
                  <Td numeric>{formatBaseUnits(w.winRateBps.toString(), 2, 2)}%</Td>
                  <Td numeric className="text-negative">
                    {formatBpsAsPercent(-w.drawdownBps)}
                  </Td>
                  <Td numeric>{formatInteger(w.trades)}</Td>
                  <Td numeric className="text-fg-secondary">
                    {formatDuration(w.lastActiveSecondsAgo)}
                  </Td>
                  <Td>
                    <StatusIndicator tone={walletStateTone[w.state] ?? 'neutral'} label={w.state} />
                  </Td>
                  <Td className="text-right">
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={(e) => {
                        e.stopPropagation();
                        setInspect(w.id);
                      }}
                    >
                      Inspect
                    </Button>
                  </Td>
                </Tr>
              ))}
            </TBody>
          </Table>
        </FixtureFrame>
      </div>
      <Drawer
        open={wallet !== undefined}
        onOpenChange={(open) => {
          if (!open) setInspect(null);
        }}
        title={wallet ? `Inspector · ${wallet.label}` : 'Inspector'}
        description="Fixture data · demo inspector"
      >
        {wallet ? (
          <DefinitionList
            items={[
              { term: 'Identifier', value: <span className="font-mono text-xs">{wallet.id}</span> },
              { term: 'Score', value: <span className="num">{wallet.score}</span> },
              {
                term: 'Trades',
                value: <span className="num">{formatInteger(wallet.trades)}</span>,
              },
              { term: 'State', value: wallet.state },
            ]}
          />
        ) : null}
      </Drawer>
    </Panel>
  );
}

function ChartTokensPanel() {
  const series: [string, string, string][] = [
    ['chart-1', 'solid', 'var(--color-chart-1)'],
    ['chart-2', '6 3 dash', 'var(--color-chart-2)'],
    ['chart-3', '2 2 dot', 'var(--color-chart-3)'],
    ['chart-4', '8 2 2 2', 'var(--color-chart-4)'],
    ['chart-5', 'solid + markers', 'var(--color-chart-5)'],
  ];
  const dash: Record<string, string | undefined> = {
    solid: undefined,
    '6 3 dash': '6 3',
    '2 2 dot': '2 2',
    '8 2 2 2': '8 2 2 2',
    'solid + markers': undefined,
  };
  return (
    <Panel title="Chart system tokens">
      <div className="flex flex-col gap-2">
        {series.map(([name, style, color]) => (
          <div key={name} className="flex items-center gap-3">
            <svg width="72" height="12" aria-hidden="true">
              <line
                x1="0"
                y1="6"
                x2="72"
                y2="6"
                stroke={color}
                strokeWidth="2"
                strokeDasharray={dash[style]}
              />
              {style === 'solid + markers' ? (
                <>
                  <rect x="14" y="3" width="6" height="6" fill={color} />
                  <rect x="50" y="3" width="6" height="6" fill={color} />
                </>
              ) : null}
            </svg>
            <span className="font-mono text-2xs text-fg">{name}</span>
            <span className="text-xs text-fg-muted">{style}</span>
          </div>
        ))}
        <p className="m-0 pt-1 text-xs text-fg-muted">
          Series differ by line style or markers as well as hue. Grid, axis and crosshair tokens:
          chart-grid, chart-axis, chart-crosshair. No chart library is installed in Phase 00.
        </p>
      </div>
    </Panel>
  );
}

function NumberFormattingPanel() {
  return (
    <Panel title="Number formatting" flush>
      <FixtureFrame className="m-3 p-0 pt-4">
        <Table label="Formatting examples">
          <THead>
            <Tr>
              <Th>Input (base units)</Th>
              <Th numeric>Decimals</Th>
              <Th numeric>Formatted</Th>
              <Th>Case</Th>
            </Tr>
          </THead>
          <TBody>
            {fixtureTokenAmounts.map((a) => (
              <Tr key={a.raw}>
                <Td className="num font-mono text-2xs">{a.raw}</Td>
                <Td numeric>{a.decimals}</Td>
                <Td numeric>{formatBaseUnits(a.raw, a.decimals, a.decimals)}</Td>
                <Td className="text-fg-muted">{a.note}</Td>
              </Tr>
            ))}
            <Tr>
              <Td className="num font-mono text-2xs">bps 125 / −40</Td>
              <Td numeric>2</Td>
              <Td numeric>
                {formatBpsAsPercent(125)} / {formatBpsAsPercent(-40)}
              </Td>
              <Td className="text-fg-muted">signed delta, U+2212 minus</Td>
            </Tr>
          </TBody>
        </Table>
      </FixtureFrame>
    </Panel>
  );
}

function GeometryPanel() {
  return (
    <Panel title="Spacing & radius">
      <div className="flex flex-col gap-3">
        <div className="flex items-end gap-3">
          {[1, 2, 3, 4, 6, 8].map((n) => (
            <div key={n} className="flex flex-col items-center gap-1">
              <span className="block bg-fg-secondary" style={{ width: n * 4, height: n * 4 }} />
              <span className="num text-2xs text-fg-muted">{n * 4}</span>
            </div>
          ))}
          <span className="pb-4 text-xs text-fg-muted">4 px grid</span>
        </div>
        <div className="flex items-center gap-3">
          {[
            ['xs', 'rounded-xs', '2'],
            ['sm', 'rounded-sm', '3'],
            ['md', 'rounded-md', '4'],
            ['lg', 'rounded-lg', '6'],
          ].map(([n, c, px]) => (
            <div key={n} className="flex flex-col items-center gap-1">
              <span className={cn('block size-10 border border-line-control bg-raised', c)} />
              <span className="text-2xs text-fg-muted">
                {n} · {px}px
              </span>
            </div>
          ))}
        </div>
      </div>
    </Panel>
  );
}
