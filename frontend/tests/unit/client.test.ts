import { afterEach, describe, expect, it, vi } from 'vitest';

import { ApiRequestError, describeFailure, getJson } from '@/lib/api/client.ts';
import { isReadyResponse, isSystemStatus } from '@/lib/api/guards.ts';

function mockFetch(impl: () => Promise<Response>) {
  vi.stubGlobal('fetch', vi.fn(impl));
}

const jsonResponse = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

async function failureOf(p: Promise<unknown>) {
  try {
    await p;
  } catch (e) {
    expect(e).toBeInstanceOf(ApiRequestError);
    return describeFailure(e);
  }
  throw new Error('expected rejection');
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('getJson', () => {
  it('classifies a network error as unreachable', async () => {
    mockFetch(() => Promise.reject(new TypeError('Failed to fetch')));
    const f = await failureOf(getJson('/x', { guard: isSystemStatus }));
    expect(f.kind).toBe('unreachable');
  });

  it('classifies a proxy 502 HTML page as unreachable, never healthy', async () => {
    mockFetch(() =>
      Promise.resolve(
        new Response('Bad Gateway', { status: 502, headers: { 'content-type': 'text/html' } }),
      ),
    );
    const f = await failureOf(getJson('/x', { guard: isSystemStatus }));
    expect(f.kind).toBe('unreachable');
  });

  it('surfaces the structured error envelope', async () => {
    mockFetch(() =>
      Promise.resolve(
        jsonResponse(404, {
          error: { code: 'NOT_FOUND', message: 'No such route.', request_id: 'req-12345678' },
        }),
      ),
    );
    const f = await failureOf(getJson('/x', { guard: isSystemStatus }));
    expect(f).toEqual({
      kind: 'http',
      status: 404,
      code: 'NOT_FOUND',
      message: 'No such route.',
      requestId: 'req-12345678',
    });
  });

  it('rejects a 200 response that does not match the contract', async () => {
    mockFetch(() => Promise.resolve(jsonResponse(200, { application: 'SOLPRINT' })));
    const f = await failureOf(getJson('/x', { guard: isSystemStatus }));
    expect(f.kind).toBe('invalid');
  });

  it('rejects malformed JSON', async () => {
    mockFetch(() =>
      Promise.resolve(
        new Response('{', { status: 200, headers: { 'content-type': 'application/json' } }),
      ),
    );
    const f = await failureOf(getJson('/x', { guard: isSystemStatus }));
    expect(f.kind).toBe('invalid');
  });

  it('accepts a 503 readiness body only when explicitly allowed', async () => {
    const body = {
      status: 'NOT_READY',
      checks: {
        database: { status: 'UNAVAILABLE', latency_us: null, checked_at: '2026-11-26T00:00:00Z' },
      },
    };
    mockFetch(() => Promise.resolve(jsonResponse(503, body)));
    const r = await getJson('/health/ready', { guard: isReadyResponse, acceptStatuses: [503] });
    expect(r).toEqual({ status: 503, data: body });
  });

  it('classifies a slow backend as a timeout', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        (_: unknown, init?: RequestInit) =>
          new Promise<Response>((_resolve, reject) => {
            init?.signal?.addEventListener('abort', () => {
              reject(new DOMException('aborted', 'TimeoutError'));
            });
          }),
      ),
    );
    const f = await failureOf(getJson('/x', { guard: isSystemStatus, timeoutMs: 10 }));
    expect(f.kind).toBe('timeout');
  });
});
