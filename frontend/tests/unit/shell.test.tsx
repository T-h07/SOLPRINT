import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { routes } from '@/app/router.tsx';
import { TooltipProvider } from '@/components/ui/Tooltip.tsx';

// Synthetic test values only.
const status = {
  application: 'SOLPRINT',
  environment: 'test',
  version: '0.1.0',
  trading: { mode: 'PAPER', live_trading_enabled: false, execution_capability: 'NONE' },
  started_at: '2026-11-26T09:00:00Z',
  timestamp: '2026-11-26T12:00:00Z',
  uptime_seconds: 10800,
  database: { status: 'AVAILABLE', latency_us: 400, migrations_applied: 1, migrations_expected: 1 },
  providers: [],
};
const ready = {
  status: 'READY',
  checks: {
    database: { status: 'AVAILABLE', latency_us: 400, checked_at: '2026-11-26T12:00:00Z' },
  },
};
const version = {
  name: 'solprint',
  version: '0.1.0',
  api_version: 'v1',
  git_commit: null,
  build_profile: 'debug',
};

function stubBackend(online: boolean) {
  vi.stubGlobal(
    'fetch',
    vi.fn((input: RequestInfo | URL) => {
      if (!online) return Promise.reject(new TypeError('Failed to fetch'));
      const url = input instanceof Request ? input.url : input.toString();
      const body = url.includes('/health/ready')
        ? ready
        : url.includes('/version')
          ? version
          : status;
      return Promise.resolve(
        new Response(JSON.stringify(body), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
      );
    }),
  );
}

function renderAt(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('application shell', () => {
  it('shows PAPER mode as reported by the backend', async () => {
    stubBackend(true);
    renderAt('/');
    const mode = await screen.findByTestId('mode-indicator');
    expect(mode).toHaveAttribute('data-mode', 'paper');
    expect(within(mode).getByText(/paper mode/i)).toBeInTheDocument();
    expect(within(mode).getByText(/live trading disabled/i)).toBeInTheDocument();
    expect(await screen.findByTestId('connection-indicator')).toHaveAttribute(
      'data-state',
      'connected',
    );
  });

  it('never assumes a mode when the backend is unreachable', async () => {
    stubBackend(false);
    renderAt('/');
    const mode = await screen.findByTestId('mode-indicator');
    expect(mode).toHaveAttribute('data-mode', 'unverified');
    expect(within(mode).queryByText(/paper mode/i)).not.toBeInTheDocument();
    expect(await screen.findByTestId('connection-indicator')).toHaveAttribute(
      'data-state',
      'offline',
    );
  });

  it('has primary navigation, a skip link and one h1 per page', async () => {
    stubBackend(true);
    renderAt('/system');
    const nav = screen.getByRole('navigation', { name: 'Primary' });
    expect(within(nav).getByRole('link', { name: /overview/i })).toBeInTheDocument();
    expect(within(nav).getByRole('link', { name: 'System' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('link', { name: /skip to workspace/i })).toHaveAttribute(
      'href',
      '#workspace',
    );
    expect(await screen.findAllByRole('heading', { level: 1 })).toHaveLength(1);
  });

  it('renders a not-found page for unknown routes', async () => {
    stubBackend(true);
    renderAt('/nope');
    expect(
      await screen.findByRole('heading', { level: 1, name: /not found/i }),
    ).toBeInTheDocument();
  });
});
