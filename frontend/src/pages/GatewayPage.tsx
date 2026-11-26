import {
  BudgetPanel,
  CachePanel,
  GatewayStatePanel,
  OperationsPanel,
  ProviderHealthPanel,
  StreamsPanel,
  UsagePanel,
} from '@/features/gateway/panels.tsx';
import { PageLayout } from '@/layouts/PageLayout.tsx';

/**
 * GATEWAY monitoring. Eye flow: budget and gateway state (what can stop
 * work) → provider health → per-operation detail → usage, streams, cache.
 */
export function GatewayPage() {
  return (
    <PageLayout title="Gateway" context="Provider access · read-only · no execution">
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <BudgetPanel className="xl:col-span-2" />
        <GatewayStatePanel />
        <ProviderHealthPanel className="xl:col-span-3" />
        <OperationsPanel className="xl:col-span-3" />
        <UsagePanel />
        <StreamsPanel />
        <CachePanel />
      </div>
    </PageLayout>
  );
}
