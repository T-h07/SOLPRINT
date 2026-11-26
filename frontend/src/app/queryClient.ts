import { QueryClient } from '@tanstack/react-query';

export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // One quick retry absorbs a transient blip; polling covers the rest.
        retry: 1,
        retryDelay: 500,
        refetchOnWindowFocus: true,
      },
    },
  });
}
