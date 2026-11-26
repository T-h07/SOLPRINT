/**
 * Typed same-origin API client.
 *
 * Response types come from `src/types/generated`, which is generated from the
 * Rust contracts (single source of truth). Failures are classified so the UI
 * can say *what* failed instead of a generic "something went wrong":
 *
 * - `unreachable`  the backend could not be reached (network error, or the
 *                  dev/nginx proxy answered 502/503/504 without a JSON body)
 * - `http`         the backend answered with a structured error envelope
 * - `invalid`      the backend answered something that is not the contract
 * - `timeout`      no answer within the client timeout
 */
import type { ApiError, ApiErrorCode } from '@/types/api.ts';

const DEFAULT_TIMEOUT_MS = 8_000;

export type ApiFailure =
  | { kind: 'unreachable'; message: string }
  | { kind: 'timeout'; message: string }
  | {
      kind: 'http';
      status: number;
      code: ApiErrorCode | 'UNKNOWN';
      message: string;
      requestId: string | null;
    }
  | { kind: 'invalid'; status: number; message: string };

export class ApiRequestError extends Error {
  readonly failure: ApiFailure;

  constructor(failure: ApiFailure) {
    super(failure.message);
    this.name = 'ApiRequestError';
    this.failure = failure;
  }
}

function isApiError(value: unknown): value is ApiError {
  if (typeof value !== 'object' || value === null || !('error' in value)) return false;
  const error = value.error;
  return (
    typeof error === 'object' &&
    error !== null &&
    typeof (error as { code?: unknown }).code === 'string' &&
    typeof (error as { message?: unknown }).message === 'string'
  );
}

export interface GetOptions<T> {
  signal?: AbortSignal | undefined;
  /** Validates the decoded JSON against the expected contract shape. */
  guard: (value: unknown) => value is T;
  /** Non-2xx statuses whose body is a contract response, not an error (e.g. 503 readiness). */
  acceptStatuses?: readonly number[];
  timeoutMs?: number;
}

export interface ApiResponse<T> {
  status: number;
  data: T;
}

export async function getJson<T>(path: string, options: GetOptions<T>): Promise<ApiResponse<T>> {
  const timeout = AbortSignal.timeout(options.timeoutMs ?? DEFAULT_TIMEOUT_MS);
  const signal = options.signal ? AbortSignal.any([options.signal, timeout]) : timeout;

  let response: Response;
  try {
    response = await fetch(path, {
      method: 'GET',
      headers: { Accept: 'application/json' },
      credentials: 'same-origin',
      signal,
    });
  } catch (cause) {
    if (timeout.aborted) {
      throw new ApiRequestError({ kind: 'timeout', message: 'Backend did not respond in time.' });
    }
    if (options.signal?.aborted) throw cause;
    throw new ApiRequestError({ kind: 'unreachable', message: 'Backend unreachable.' });
  }

  const isJson = response.headers.get('content-type')?.startsWith('application/json') ?? false;
  let body: unknown = null;
  if (isJson) {
    try {
      body = await response.json();
    } catch {
      throw new ApiRequestError({
        kind: 'invalid',
        status: response.status,
        message: 'Backend returned malformed JSON.',
      });
    }
  }

  const accepted = response.ok || (options.acceptStatuses?.includes(response.status) ?? false);
  if (accepted && options.guard(body)) {
    return { status: response.status, data: body };
  }

  if (!response.ok) {
    if (isApiError(body)) {
      throw new ApiRequestError({
        kind: 'http',
        status: response.status,
        code: body.error.code,
        message: body.error.message,
        requestId: body.error.request_id,
      });
    }
    if (response.status >= 500) {
      // A proxy (Vite in development, nginx in containers) answering for a
      // backend it cannot reach.
      throw new ApiRequestError({ kind: 'unreachable', message: 'Backend unreachable.' });
    }
    throw new ApiRequestError({
      kind: 'http',
      status: response.status,
      code: 'UNKNOWN',
      message: `Request failed (${response.status}).`,
      requestId: null,
    });
  }

  throw new ApiRequestError({
    kind: 'invalid',
    status: response.status,
    message: 'Backend response does not match the expected contract.',
  });
}

export function describeFailure(error: unknown): ApiFailure {
  if (error instanceof ApiRequestError) return error.failure;
  return { kind: 'unreachable', message: 'Backend unreachable.' };
}
