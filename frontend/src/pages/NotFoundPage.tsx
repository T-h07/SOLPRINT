import { Link } from 'react-router';

import { DataState } from '@/components/ui/DataState.tsx';
import { PageLayout } from '@/layouts/PageLayout.tsx';

export function NotFoundPage() {
  return (
    <PageLayout title="Not found">
      <div className="rounded-md border border-line bg-base p-3">
        <DataState
          kind="empty"
          title="This route does not exist in the current build."
          detail={
            <Link to="/" className="text-fg-secondary underline underline-offset-2 hover:text-fg">
              Return to Overview
            </Link>
          }
        />
      </div>
    </PageLayout>
  );
}
