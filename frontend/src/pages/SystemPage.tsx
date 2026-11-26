import {
  BuildPanel,
  DatabasePanel,
  ProvidersPanel,
  ReadinessPanel,
  RuntimePanel,
} from '@/features/system/panels.tsx';
import { PageLayout } from '@/layouts/PageLayout.tsx';

export function SystemPage() {
  return (
    <PageLayout title="System" context="Application, database and provider state">
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2 2xl:grid-cols-3">
        <ReadinessPanel />
        <DatabasePanel />
        <RuntimePanel />
        <BuildPanel />
        <ProvidersPanel className="lg:col-span-2" />
      </div>
    </PageLayout>
  );
}
