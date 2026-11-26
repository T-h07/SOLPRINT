import { expect, test } from '@playwright/test';

/**
 * End-to-end against the REAL running stack (no mocks): frontend, backend
 * and PostgreSQL. Start it first (scripts/dev.ps1 -Mode docker) and set
 * E2E_BASE_URL if it is not http://127.0.0.1:3000.
 */
test('the shell reflects the real backend: PAPER, ready, no execution', async ({ page, request }) => {
  const status = await request.get('/api/v1/system/status');
  expect(status.ok()).toBe(true);
  const body = (await status.json()) as {
    trading: { mode: string; live_trading_enabled: boolean; execution_capability: string };
  };
  expect(body.trading).toEqual({
    mode: 'PAPER',
    live_trading_enabled: false,
    execution_capability: 'NONE',
  });

  await page.goto('/');
  await expect(page.getByTestId('mode-indicator')).toHaveAttribute('data-mode', 'paper');
  await expect(page.getByTestId('connection-indicator')).toHaveAttribute('data-state', 'connected');
});

test('readiness and liveness answer through the same-origin proxy', async ({ request }) => {
  const live = await request.get('/health/live');
  expect(live.status()).toBe(200);
  const ready = await request.get('/health/ready');
  expect(ready.status()).toBe(200);
  expect(((await ready.json()) as { status: string }).status).toBe('READY');
});

test('system page shows real database and provider state', async ({ page }) => {
  await page.goto('/system');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('System');
  await expect(page.getByText(/not initialized/i).first()).toBeVisible();
  await expect(page.getByText(/available/i).first()).toBeVisible();
});

test('production build does not expose the design-system showcase', async ({ page }) => {
  await page.goto('/design-system');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Not found');
});
