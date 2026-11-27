import { lazy, Suspense } from 'react';
import { createBrowserRouter, type RouteObject } from 'react-router';

import { AppShell } from '@/components/shell/AppShell.tsx';
import { DataPage } from '@/pages/DataPage.tsx';
import { GatewayPage } from '@/pages/GatewayPage.tsx';
import { NotFoundPage } from '@/pages/NotFoundPage.tsx';
import { OverviewPage } from '@/pages/OverviewPage.tsx';
import { SystemPage } from '@/pages/SystemPage.tsx';

import { RouteError } from './ErrorBoundary.tsx';

// Code-split and only referenced when enabled, so production bundles do not
// ship the showcase or its fixtures.
// The condition is written inline (not via flags.ts) so the bundler can
// statically eliminate the dynamic import when it is false.
const DesignSystemPage =
  import.meta.env.DEV ||
  import.meta.env.MODE === 'showcase' ||
  import.meta.env.VITE_ENABLE_DESIGN_SYSTEM === 'true'
    ? lazy(() => import('@/pages/DesignSystemPage.tsx'))
    : null;

export const routes: RouteObject[] = [
  {
    path: '/',
    element: <AppShell />,
    errorElement: <RouteError />,
    children: [
      { index: true, element: <OverviewPage /> },
      { path: 'gateway', element: <GatewayPage /> },
      { path: 'data', element: <DataPage /> },
      { path: 'system', element: <SystemPage /> },
      ...(DesignSystemPage
        ? [
            {
              path: 'design-system',
              element: (
                <Suspense fallback={null}>
                  <DesignSystemPage />
                </Suspense>
              ),
            },
          ]
        : []),
      { path: '*', element: <NotFoundPage /> },
    ],
  },
];

export function createAppRouter() {
  return createBrowserRouter(routes);
}
