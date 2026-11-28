import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';

import { mockBackend, settle, VIEWPORTS } from '../support/mockApi.ts';

/** Collect console errors and uncaught exceptions for the whole test. */
function trackErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('console', (m) => {
    // The offline scenario intentionally produces failed (502) requests.
    if (m.type() === 'error' && !m.text().includes('502')) errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(e.message));
  return errors;
}

async function axe(page: Page) {
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'])
    .analyze();
  return result.violations.map((v) => `${v.id}: ${v.nodes.length.toString()} node(s)`);
}

test.describe('operating state', () => {
  test('healthy backend: PAPER mode, ready, no console errors', async ({ page }) => {
    const errors = trackErrors(page);
    await mockBackend(page, 'healthy');
    await page.goto('/');
    await settle(page);

    const mode = page.getByTestId('mode-indicator');
    await expect(mode).toHaveAttribute('data-mode', 'paper');
    await expect(mode).toContainText(/paper mode/i);
    await expect(mode).toContainText(/live trading disabled/i);
    await expect(page.getByTestId('connection-indicator')).toHaveAttribute(
      'data-state',
      'connected',
    );
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Overview');
    expect(errors).toEqual([]);
  });

  test('backend offline: mode is UNVERIFIED, never assumed PAPER', async ({ page }) => {
    await mockBackend(page, 'offline');
    await page.goto('/');
    const mode = page.getByTestId('mode-indicator');
    await expect(mode).toHaveAttribute('data-mode', 'unverified');
    await expect(mode).not.toContainText(/paper mode/i);
    await expect(page.getByTestId('connection-indicator')).toHaveAttribute('data-state', 'offline');
  });

  test('database down: degraded, readiness explains why', async ({ page }) => {
    await mockBackend(page, 'database-down');
    await page.goto('/system');
    await settle(page);
    await expect(page.getByTestId('connection-indicator')).toHaveAttribute(
      'data-state',
      'degraded',
    );
    await expect(page.getByText(/unavailable/i).first()).toBeVisible();
  });

  test('loading: skeletons, no fabricated values', async ({ page }) => {
    await mockBackend(page, 'loading');
    await page.goto('/');
    await expect(page.locator('[aria-busy="true"]').first()).toBeVisible();
    await expect(page.getByTestId('connection-indicator')).toHaveAttribute(
      'data-state',
      'connecting',
    );
    await expect(page.getByText(/paper mode/i)).toHaveCount(0);
  });

  test('provider health comes from GATEWAY, unmeasured providers say so', async ({ page }) => {
    await mockBackend(page, 'healthy');
    await page.goto('/system');
    await settle(page);
    await expect(page.getByText('Helius').first()).toBeVisible();
    await expect(page.getByText(/^healthy$/i).first()).toBeVisible();
    await expect(page.getByText(/not measured/i).first()).toBeVisible();
  });
});

test.describe('navigation and keyboard', () => {
  test('skip link, tab order and route change by keyboard', async ({ page }) => {
    await mockBackend(page, 'healthy');
    await page.goto('/');
    await settle(page);

    await page.keyboard.press('Tab');
    const skip = page.getByRole('link', { name: /skip to workspace/i });
    await expect(skip).toBeFocused();
    await expect(skip).toBeVisible();

    const nav = page.getByRole('navigation', { name: 'Primary' });
    const systemLink = nav.getByRole('link', { name: 'System', exact: true });
    await systemLink.focus();
    await expect(systemLink).toBeFocused();
    // Focus must be visible (focus ring), not just programmatically present.
    const outline = await systemLink.evaluate((el) => getComputedStyle(el).outlineStyle);
    expect(outline).not.toBe('none');
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/system$/);
    await expect(systemLink).toHaveAttribute('aria-current', 'page');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('System');
  });

  test('unknown route renders not-found inside the shell', async ({ page }) => {
    await mockBackend(page, 'healthy');
    await page.goto('/does-not-exist');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Not found');
    await expect(page.getByTestId('mode-indicator')).toBeVisible();
  });
});

test.describe('accessibility (axe, WCAG 2.2 AA)', () => {
  for (const path of [
    '/',
    '/gateway',
    '/data',
    '/radar',
    '/system',
    '/design-system',
    '/does-not-exist',
  ]) {
    test(`no violations on ${path}`, async ({ page }) => {
      await mockBackend(page, 'healthy');
      await page.goto(path);
      await settle(page, { allowBusy: path === '/design-system' });
      expect(await axe(page)).toEqual([]);
    });
  }

  for (const scenario of ['gateway-idle', 'gateway-pressure', 'offline', 'loading'] as const) {
    test(`no violations on /gateway (${scenario})`, async ({ page }) => {
      await mockBackend(page, scenario);
      await page.goto('/gateway');
      await settle(page, { allowBusy: scenario === 'offline' || scenario === 'loading' });
      expect(await axe(page)).toEqual([]);
    });
  }

  test('no violations in the offline state', async ({ page }) => {
    await mockBackend(page, 'offline');
    await page.goto('/');
    await expect(page.getByTestId('mode-indicator')).toHaveAttribute('data-mode', 'unverified');
    expect(await axe(page)).toEqual([]);
  });
});

test.describe('responsive layout', () => {
  for (const vp of VIEWPORTS) {
    for (const path of ['/', '/gateway', '/data', '/radar', '/system', '/design-system']) {
      test(`no horizontal overflow at ${vp.name} on ${path}`, async ({ page }) => {
        await page.setViewportSize({ width: vp.width, height: vp.height });
        await mockBackend(page, 'healthy');
        await page.goto(path);
        await settle(page, { allowBusy: path === '/design-system' });

        const overflow = await page.evaluate(
          () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
        );
        expect(overflow).toBeLessThanOrEqual(0);
        // Safety-critical state stays visible at every width.
        await expect(page.getByTestId('mode-indicator')).toBeInViewport();
        await expect(page.getByTestId('connection-indicator')).toBeInViewport();
      });
    }
  }
});

test.describe('gateway workspace', () => {
  test('budget, providers and operations render from the API', async ({ page }) => {
    await mockBackend(page, 'healthy');
    await page.goto('/gateway');
    await settle(page);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Gateway');
    const meter = page.getByRole('meter', { name: 'Helius credits used' });
    await expect(meter).toHaveAttribute('aria-valuenow', '212904');
    await expect(page.getByText('Conservation ≥ 560,000')).toBeVisible();
    await expect(page.getByRole('cell', { name: 'getSignaturesForAddress' })).toBeVisible();
    // Read-only is stated, not implied.
    await expect(page.getByText(/read-only access layer/i)).toBeVisible();
    // Local accounting is labelled as an estimate, not the provider's bill.
    await expect(page.getByText(/authoritative for billing/i)).toBeVisible();
  });

  test('idle gateway says nothing was measured instead of showing zeros as health', async ({
    page,
  }) => {
    await mockBackend(page, 'gateway-idle');
    await page.goto('/gateway');
    await settle(page);
    await expect(page.getByText(/no provider request has been made/i)).toBeVisible();
    await expect(page.getByText(/streams are off/i)).toBeVisible();
    await expect(page.getByText(/not measured/i).first()).toBeVisible();
  });

  test('pressure: conservation, open circuit and stream gap are spelled out', async ({ page }) => {
    await mockBackend(page, 'gateway-pressure');
    await page.goto('/gateway');
    await settle(page);
    await expect(page.getByText('Conservation', { exact: true }).first()).toBeVisible();
    await expect(page.getByText('P0–P3')).toBeVisible();
    await expect(page.getByText(/^open$/i).first()).toBeVisible();
    await expect(page.getByText(/^gap$/i).first()).toBeVisible();
    await expect(page.getByText(/118 dropped/)).toBeVisible();
  });

  test('database down: stored usage is marked partial, not empty', async ({ page }) => {
    await mockBackend(page, 'database-down');
    await page.goto('/gateway');
    await settle(page);
    await expect(page.getByText(/stored history unavailable/i)).toBeVisible();
  });

  test('offline: every pane reports the failure with a safe retry', async ({ page }) => {
    await mockBackend(page, 'offline');
    await page.goto('/gateway');
    await expect(page.getByText(/credit budget unavailable/i)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Retry' }).first()).toBeVisible();
  });
});

test.describe('data engine workspace', () => {
  test('swaps render exact amounts, attribution and finality as text', async ({ page }) => {
    const errors = trackErrors(page);
    await mockBackend(page, 'healthy');
    await page.goto('/data');
    await settle(page);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Data engine');
    const swaps = page.getByRole('region', { name: 'Normalized swaps, newest first' });
    // BigInt formatting: 0.5 SOL in, 17,000,000 tokens (6 decimals) out.
    await expect(swaps.getByText('0.5000').filter({ visible: true }).first()).toBeVisible();
    await expect(
      swaps.getByText('17,000,000.0000').filter({ visible: true }).first(),
    ).toBeVisible();
    // Unknown output amount is a dash, never zero.
    await expect(swaps.getByText('Insufficient evidence')).toBeVisible();
    // Program-controlled swap carries no wallet; finality states are words.
    await expect(swaps.getByText('Program', { exact: true })).toBeVisible();
    await expect(swaps.getByText('Finality unknown')).toBeVisible();
    await expect(swaps.getByText(/retracted/i)).toBeVisible();
    // Read-only and no USD valuation are stated.
    await expect(page.getByText(/read-only decoding/i)).toBeVisible();
    await expect(page.getByText(/no USD values are shown/i)).toBeVisible();
    expect(errors).toEqual([]);
  });

  test('decoder coverage lists every decoded program, even with the database down', async ({
    page,
  }) => {
    await mockBackend(page, 'database-down');
    await page.goto('/data');
    await settle(page);
    const coverage = page.getByRole('region', { name: 'Decoded programs' });
    await expect(coverage.locator('tbody tr')).toHaveCount(7);
    for (const name of [
      'Jupiter',
      'Pump.fun',
      'PumpSwap',
      'Raydium AMM v4',
      'Raydium CPMM',
      'Raydium CLMM',
      'Orca Whirlpool',
    ]) {
      await expect(coverage.getByText(name, { exact: true })).toBeVisible();
    }
    await expect(page.getByText(/stored with their evidence as unsupported/i)).toBeVisible();
    await expect(page.getByText('de-2026.11.1').first()).toBeVisible();
  });

  test('protocol filter narrows the swap list', async ({ page }) => {
    await mockBackend(page, 'healthy');
    await page.goto('/data');
    await settle(page);
    await page.getByLabel('Filter swaps by protocol').selectOption('PUMP_FUN');
    const swaps = page.getByRole('region', { name: 'Normalized swaps, newest first' });
    await expect(swaps.locator('tbody tr')).toHaveCount(1);
    await expect(swaps.getByText('Pump.fun').first()).toBeAttached();
  });

  test('inspector opens by keyboard, shows evidence and returns focus on Esc', async ({ page }) => {
    await mockBackend(page, 'healthy');
    await page.goto('/data');
    await settle(page);
    const trigger = page
      .getByRole('region', { name: 'Decoded transactions, newest first' })
      .getByRole('button')
      .first();
    await trigger.focus();
    await page.keyboard.press('Enter');
    const dialog = page.getByRole('dialog', { name: 'Transaction' });
    await expect(dialog).toBeVisible();
    await expect(dialog.locator('[aria-busy="true"]')).toHaveCount(0);
    await expect(dialog.getByText(/sha256/)).toBeVisible();
    // Signed SOL delta with U+2212 minus.
    await expect(dialog.getByText('−0.502105000')).toBeVisible();
    await expect(dialog.getByText(/inferred created/i)).toBeVisible();
    expect(await axe(page)).toEqual([]);
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
  });

  test('empty engine explains why nothing is shown', async ({ page }) => {
    await mockBackend(page, 'data-empty');
    await page.goto('/data');
    await settle(page);
    await expect(page.getByText('No swaps recorded yet.')).toBeVisible();
    await expect(page.getByText('No ingestion targets configured.')).toBeVisible();
    await expect(page.getByText(/none are created automatically/i)).toBeVisible();
    expect(await axe(page)).toEqual([]);
  });

  test('database down: totals are partial and lists name the database', async ({ page }) => {
    await mockBackend(page, 'database-down');
    await page.goto('/data');
    await settle(page);
    await expect(page.getByText(/stored totals unavailable/i)).toBeVisible();
    await expect(
      page.getByText(/swaps unavailable: the backend could not read its database/i),
    ).toBeVisible();
    await expect(page.getByText('database unavailable').first()).toBeVisible();
    expect(await axe(page)).toEqual([]);
  });

  test('offline: panes report the backend failure with a safe retry', async ({ page }) => {
    await mockBackend(page, 'offline');
    await page.goto('/data');
    await expect(
      page.getByText(/data engine state unavailable: backend unreachable/i),
    ).toBeVisible();
    await expect(page.getByRole('button', { name: 'Retry' }).first()).toBeVisible();
  });

  test('loading: skeletons only, no values', async ({ page }) => {
    await mockBackend(page, 'loading');
    await page.goto('/data');
    await expect(page.locator('[aria-busy="true"]').first()).toBeVisible();
    await expect(page.getByText('0.5000')).toHaveCount(0);
    expect(await axe(page)).toEqual([]);
  });
});

test.describe('radar workspace', () => {
  test('registry renders states as text and priority with its disclaimer', async ({ page }) => {
    const errors = trackErrors(page);
    await mockBackend(page, 'healthy');
    await page.goto('/radar');
    await settle(page);
    const registry = page.getByRole('region', { name: 'Wallet registry' });
    await expect(registry.locator('tbody tr')).toHaveCount(9);
    await expect(registry.getByText('71.3', { exact: true })).toBeVisible();
    await expect(registry.locator('tbody').getByText('Monitored').first()).toBeVisible();
    await expect(page.getByText(/not a prediction, a recommendation/i)).toBeVisible();
    // Monitored wallets are refreshed from history; live monitoring is not claimed.
    await expect(
      page
        .getByText(/periodic refresh/i)
        .filter({ visible: true })
        .first(),
    ).toBeVisible();
    expect(errors).toEqual([]);
  });

  test('lifecycle filter narrows the registry', async ({ page }) => {
    await mockBackend(page, 'healthy');
    await page.goto('/radar');
    await settle(page);
    await page.getByLabel('Filter wallets by lifecycle').selectOption('REJECTED');
    await settle(page);
    const registry = page.getByRole('region', { name: 'Wallet registry' });
    await expect(registry.locator('tbody tr')).toHaveCount(2);
  });

  test('wallet inspector opens by keyboard, explains the score and returns focus on Esc', async ({
    page,
  }) => {
    await mockBackend(page, 'healthy');
    await page.goto('/radar');
    await settle(page);
    const trigger = page
      .getByRole('region', { name: 'Wallet registry' })
      .getByRole('button')
      .first();
    await trigger.focus();
    await page.keyboard.press('Enter');
    const dialog = page.getByRole('dialog', { name: 'Wallet' });
    await expect(dialog).toBeVisible();
    await expect(dialog.locator('[aria-busy="true"]')).toHaveCount(0);
    await expect(dialog.getByText('Priority components')).toBeVisible();
    await expect(dialog.getByRole('heading', { name: /matched outcomes/i })).toBeVisible();
    await expect(dialog.getByText(/retracted orphaned|retracted/i).first()).toBeVisible();
    expect(await axe(page)).toEqual([]);
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
  });

  test('empty registry explains how wallets get discovered', async ({ page }) => {
    await mockBackend(page, 'radar-empty');
    await page.goto('/radar');
    await settle(page);
    await expect(page.getByText('No wallets discovered yet.')).toBeVisible();
    await expect(page.getByText('No markets recorded.')).toBeVisible();
    await expect(page.getByText('No discovery cycle has run.').first()).toBeVisible();
    expect(await axe(page)).toEqual([]);
  });

  test('paused discovery and conservation budget are spelled out', async ({ page }) => {
    await mockBackend(page, 'radar-paused');
    await page.goto('/radar');
    await settle(page);
    await expect(page.getByText(/paused/i).first()).toBeVisible();
    await expect(page.getByText('Conservation').first()).toBeVisible();
    expect(await axe(page)).toEqual([]);
  });

  test('database down: panes name the database instead of showing empty', async ({ page }) => {
    await mockBackend(page, 'database-down');
    await page.goto('/radar');
    await settle(page);
    await expect(
      page.getByText(/wallet registry unavailable: the backend could not read its database/i),
    ).toBeVisible();
    await expect(page.getByText('No wallets discovered yet.')).toHaveCount(0);
    expect(await axe(page)).toEqual([]);
  });

  test('offline: panes report the backend failure with a safe retry', async ({ page }) => {
    await mockBackend(page, 'offline');
    await page.goto('/radar');
    await expect(page.getByText(/radar state unavailable: backend unreachable/i)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Retry' }).first()).toBeVisible();
  });

  test('loading: skeletons only, no values', async ({ page }) => {
    await mockBackend(page, 'loading');
    await page.goto('/radar');
    await expect(page.locator('[aria-busy="true"]').first()).toBeVisible();
    await expect(page.getByText('71.3')).toHaveCount(0);
    expect(await axe(page)).toEqual([]);
  });
});
