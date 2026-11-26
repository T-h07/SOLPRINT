import {
  HealthPanel,
  ModulesPanel,
  OperatingStatePanel,
  ProvidersPanel,
} from '@/features/system/panels.tsx';
import { PageLayout } from '@/layouts/PageLayout.tsx';

/**
 * Foundation overview. Eye flow: safety (operating state) → health →
 * capabilities (modules, providers). Portfolio, opportunities and market
 * context have no data source yet, so they are not drawn at all rather than
 * filled with placeholders.
 */
export function OverviewPage() {
  return (
    <PageLayout title="Overview" context="Foundation · Phase 00">
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-12">
        <OperatingStatePanel className="xl:col-span-7" />
        <HealthPanel className="xl:col-span-5" />
        <ModulesPanel className="xl:col-span-7" />
        <ProvidersPanel className="xl:col-span-5" showRole={false} />
      </div>
    </PageLayout>
  );
}
