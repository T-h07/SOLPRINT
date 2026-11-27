import { useState } from 'react';

import {
  BackfillsPanel,
  EngineStatePanel,
  EventsPanel,
  IngestionPanel,
  QualityPanel,
  SwapsPanel,
  TargetsPanel,
  TransactionsPanel,
} from '@/features/data/panels.tsx';
import { TransactionInspector } from '@/features/data/TransactionInspector.tsx';
import { PageLayout } from '@/layouts/PageLayout.tsx';

/**
 * DATA ENGINE workspace. Eye flow: ingestion totals and engine state (is
 * data arriving, is anything blocked) → swaps (the normalized output) →
 * transactions and data quality → targets, backfills and domain events.
 */
export function DataPage() {
  const [protocol, setProtocol] = useState<string | null>(null);
  const [inspected, setInspected] = useState<string | null>(null);
  return (
    <PageLayout title="Data engine" context="Chain data · read-only · no execution">
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <IngestionPanel className="xl:col-span-2" />
        <EngineStatePanel />
        <SwapsPanel
          className="xl:col-span-3"
          protocol={protocol}
          onProtocol={setProtocol}
          onOpen={setInspected}
        />
        <TransactionsPanel className="xl:col-span-2" onOpen={setInspected} />
        <QualityPanel />
        <TargetsPanel className="xl:col-span-2" />
        <BackfillsPanel />
        <EventsPanel className="xl:col-span-3" />
      </div>
      <TransactionInspector
        signature={inspected}
        onClose={() => {
          setInspected(null);
        }}
      />
    </PageLayout>
  );
}
