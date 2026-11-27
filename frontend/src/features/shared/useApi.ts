/** Query-state adapter shared by monitoring panes. */
import type { UseQueryResult } from '@tanstack/react-query';

import { type ApiResponse, describeFailure } from '@/lib/api/client.ts';

export function useApi<T>(q: UseQueryResult<ApiResponse<T>>) {
  return {
    data: q.data?.data,
    loading: q.isPending && !q.error,
    error: q.error ? describeFailure(q.error) : null,
    updatedAt: q.dataUpdatedAt,
    retry: () => void q.refetch(),
  };
}
