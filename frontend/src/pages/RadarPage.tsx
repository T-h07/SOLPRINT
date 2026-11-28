import { useState } from 'react';

import {
  BudgetPanel,
  CyclesPanel,
  DiscoveryPanel,
  JobsPanel,
  MarketsPanel,
  MonitoringPanel,
  SourcesPanel,
  WalletsPanel,
} from '@/features/radar/panels.tsx';
import { WalletInspector } from '@/features/radar/WalletInspector.tsx';
import type { WalletSortKey } from '@/lib/api/queries.ts';
import { PageLayout } from '@/layouts/PageLayout.tsx';

/**
 * RADAR workspace. Eye flow: is discovery running and may it spend credits
 * (discovery state, budget admission) → the wallet registry (the output) →
 * where wallets come from (markets, monitoring) → the work behind it (queue,
 * cycles, per-source yield).
 */
export function RadarPage() {
  const [lifecycle, setLifecycle] = useState<string | null>(null);
  const [sort, setSort] = useState<WalletSortKey>('PRIORITY');
  const [inspected, setInspected] = useState<string | null>(null);
  return (
    <PageLayout title="Radar" context="Wallet discovery · read-only · no execution">
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <DiscoveryPanel className="xl:col-span-2" />
        <BudgetPanel />
        <WalletsPanel
          className="xl:col-span-3"
          lifecycle={lifecycle}
          onLifecycle={setLifecycle}
          sort={sort}
          onSort={setSort}
          onOpen={setInspected}
        />
        <MarketsPanel className="xl:col-span-2" />
        <MonitoringPanel />
        <JobsPanel className="xl:col-span-2" />
        <CyclesPanel />
        <SourcesPanel className="xl:col-span-3" />
      </div>
      <WalletInspector
        address={inspected}
        onClose={() => {
          setInspected(null);
        }}
      />
    </PageLayout>
  );
}
