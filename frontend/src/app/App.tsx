import { QueryClientProvider } from '@tanstack/react-query';
import { useState } from 'react';
import { RouterProvider } from 'react-router/dom';

import { TooltipProvider } from '@/components/ui/Tooltip.tsx';

import { AppErrorBoundary } from './ErrorBoundary.tsx';
import { createQueryClient } from './queryClient.ts';
import { createAppRouter } from './router.tsx';

export function App() {
  const [queryClient] = useState(createQueryClient);
  const [router] = useState(createAppRouter);
  return (
    <AppErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={300} skipDelayDuration={100}>
          <RouterProvider router={router} />
        </TooltipProvider>
      </QueryClientProvider>
    </AppErrorBoundary>
  );
}
