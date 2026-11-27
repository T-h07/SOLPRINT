/**
 * Deterministic API fixtures for the `ui` and `visual` Playwright projects.
 * Shapes match the generated contract types, so a contract change that is
 * not reflected here fails typechecking. Clearly synthetic: these values
 * exist only in tests.
 */
import type { Page, Route } from '@playwright/test';

import type { ReadyResponse, SystemStatus, VersionInfo } from '../../src/types/api.ts';
import {
  gatewayBudgets,
  gatewayCache,
  gatewayMetrics,
  gatewayProviders,
  gatewayStatus,
  gatewayStreams,
  gatewayUsage,
  type GatewayVariant,
} from './gatewayFixtures.ts';
import {
  dataBackfills,
  dataDecoders,
  dataEvents,
  dataIssues,
  dataStatus,
  dataSwaps,
  dataTargets,
  dataTransaction,
  dataTransactions,
  type DataVariant,
} from './dataFixtures.ts';

export const FIXED_NOW = Date.parse('2026-11-26T12:00:00.000Z');

export const statusFixture: SystemStatus = {
  application: 'SOLPRINT',
  environment: 'development',
  version: '0.1.0',
  trading: { mode: 'PAPER', live_trading_enabled: false, execution_capability: 'NONE' },
  started_at: '2026-11-26T09:14:05.000Z',
  timestamp: '2026-11-26T12:00:00.000Z',
  uptime_seconds: 9955,
  database: {
    status: 'AVAILABLE',
    latency_us: 412,
    migrations_applied: 2,
    migrations_expected: 2,
  },
  providers: [
    {
      id: 'HELIUS',
      name: 'Helius',
      role: 'Primary RPC and WebSocket',
      state: 'HEALTHY',
      credential: 'CONFIGURED',
      last_checked_at: '2026-11-26T11:59:58.412Z',
    },
    {
      id: 'JUPITER',
      name: 'Jupiter',
      role: 'Price and routing',
      state: 'HEALTHY',
      credential: 'ABSENT',
      last_checked_at: '2026-11-26T11:59:58.412Z',
    },
    {
      id: 'DEX_SCREENER',
      name: 'DEX Screener',
      role: 'Market data',
      state: 'DEGRADED',
      credential: 'NOT_REQUIRED',
      last_checked_at: '2026-11-26T11:58:03.000Z',
    },
    {
      id: 'SOLANA_RPC',
      name: 'Solana RPC',
      role: 'Fallback RPC',
      state: 'UNKNOWN',
      credential: 'NOT_REQUIRED',
      last_checked_at: null,
    },
  ],
};

const readyFixture: ReadyResponse = {
  status: 'READY',
  checks: {
    database: { status: 'AVAILABLE', latency_us: 412, checked_at: '2026-11-26T12:00:00.000Z' },
  },
};

const notReadyFixture: ReadyResponse = {
  status: 'NOT_READY',
  checks: {
    database: { status: 'UNAVAILABLE', latency_us: null, checked_at: '2026-11-26T12:00:00.000Z' },
  },
};

export const versionFixture: VersionInfo = {
  name: 'solprint',
  version: '0.1.0',
  api_version: 'v1',
  git_commit: '0123456789abcdef0123456789abcdef01234567',
  build_profile: 'release',
};

export type Scenario =
  | 'healthy'
  | 'database-down'
  | 'offline'
  | 'loading'
  | 'gateway-idle'
  | 'gateway-pressure'
  | 'data-empty';

/** Scenarios that only change one workspace answer like `healthy` elsewhere. */
const base = (s: Scenario): 'healthy' | 'database-down' | 'offline' | 'loading' =>
  s === 'gateway-idle' || s === 'gateway-pressure' || s === 'data-empty' ? 'healthy' : s;

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });

/** Freeze the clock and route every API call to the chosen scenario. */
export async function mockBackend(page: Page, full: Scenario): Promise<void> {
  await page.clock.setFixedTime(FIXED_NOW);
  await mockGateway(page, full);
  await mockData(page, full);
  const scenario = base(full);

  await page.route('**/health/ready', (route) => {
    switch (scenario) {
      case 'healthy':
        return json(route, 200, readyFixture);
      case 'database-down':
        return json(route, 503, notReadyFixture);
      case 'offline':
        // What the Vite/nginx proxy returns when the backend is down.
        return route.fulfill({ status: 502, contentType: 'text/html', body: 'Bad Gateway' });
      case 'loading':
        return new Promise<void>(() => undefined);
    }
  });

  await page.route('**/api/v1/system/status', (route) => {
    switch (scenario) {
      case 'healthy':
        return json(route, 200, statusFixture);
      case 'database-down':
        return json(route, 200, {
          ...statusFixture,
          database: {
            status: 'UNAVAILABLE',
            latency_us: null,
            migrations_applied: null,
            migrations_expected: 2,
          },
        } satisfies SystemStatus);
      case 'offline':
        return route.fulfill({ status: 502, contentType: 'text/html', body: 'Bad Gateway' });
      case 'loading':
        return new Promise<void>(() => undefined);
    }
  });

  await page.route('**/api/v1/system/version', (route) => {
    if (scenario === 'offline') {
      return route.fulfill({ status: 502, contentType: 'text/html', body: 'Bad Gateway' });
    }
    if (scenario === 'loading') return new Promise<void>(() => undefined);
    return json(route, 200, versionFixture);
  });
}

async function mockGateway(page: Page, scenario: Scenario): Promise<void> {
  const variant: GatewayVariant =
    scenario === 'gateway-idle' ? 'idle' : scenario === 'gateway-pressure' ? 'pressure' : 'active';
  const bodies: Record<string, () => unknown> = {
    status: () => gatewayStatus(variant),
    providers: () => gatewayProviders(variant),
    budgets: () => gatewayBudgets(variant),
    metrics: () => gatewayMetrics(variant),
    streams: () => gatewayStreams(variant),
    cache: () => gatewayCache(variant),
    usage: () => gatewayUsage(variant, scenario !== 'database-down'),
  };
  await page.route('**/api/v1/gateway/**', (route) => {
    if (scenario === 'offline') {
      return route.fulfill({ status: 502, contentType: 'text/html', body: 'Bad Gateway' });
    }
    if (scenario === 'loading') return new Promise<void>(() => undefined);
    const part = new URL(route.request().url()).pathname.split('/').pop() ?? '';
    const body = bodies[part];
    return body ? json(route, 200, body()) : json(route, 404, {});
  });
}

async function mockData(page: Page, scenario: Scenario): Promise<void> {
  const variant: DataVariant =
    scenario === 'data-empty' ? 'empty' : scenario === 'database-down' ? 'partial' : 'active';
  await page.route('**/api/v1/data/**', (route) => {
    if (scenario === 'offline') {
      return route.fulfill({ status: 502, contentType: 'text/html', body: 'Bad Gateway' });
    }
    if (scenario === 'loading') return new Promise<void>(() => undefined);
    const url = new URL(route.request().url());
    const parts = url.pathname.split('/').slice(4);
    const unavailable = () =>
      json(route, 503, {
        error: { code: 'SERVICE_UNAVAILABLE', message: 'Service unavailable.', request_id: null },
      });
    switch (parts[0]) {
      case 'status':
        return json(route, 200, dataStatus(variant));
      case 'swaps':
        return variant === 'partial'
          ? unavailable()
          : json(route, 200, dataSwaps(variant, url.searchParams.get('protocol')));
      case 'transactions': {
        if (variant === 'partial') return unavailable();
        if (parts[1]) {
          const d = dataTransaction(parts[1]);
          return d
            ? json(route, 200, d)
            : json(route, 404, {
                error: { code: 'NOT_FOUND', message: 'Resource not found.', request_id: null },
              });
        }
        return json(route, 200, dataTransactions(variant));
      }
      case 'issues':
        return variant === 'partial' ? unavailable() : json(route, 200, dataIssues(variant));
      case 'targets':
        return variant === 'partial' ? unavailable() : json(route, 200, dataTargets(variant));
      case 'backfills':
        return variant === 'partial' ? unavailable() : json(route, 200, dataBackfills(variant));
      case 'events':
        return variant === 'partial' ? unavailable() : json(route, 200, dataEvents(variant));
      case 'decoders':
        return json(route, 200, dataDecoders());
      default:
        return json(route, 404, {});
    }
  });
}

/** Wait until fonts are loaded and no request-driven skeleton remains. */
export async function settle(page: Page, opts: { allowBusy?: boolean } = {}): Promise<void> {
  // Lazy routes (the design-system showcase) render nothing until their chunk
  // loads; every page has exactly one h1 once it is mounted.
  await page.getByRole('heading', { level: 1 }).waitFor();
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  if (!opts.allowBusy) {
    await page.waitForFunction(() => document.querySelectorAll('[aria-busy="true"]').length === 0);
  }
}

export const VIEWPORTS = [
  { name: '1920x1080', width: 1920, height: 1080 },
  { name: '1440x900', width: 1440, height: 900 },
  { name: '1280x800', width: 1280, height: 800 },
  { name: '1024x768', width: 1024, height: 768 },
  { name: '390x844', width: 390, height: 844 },
] as const;
