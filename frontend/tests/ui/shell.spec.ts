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
  for (const path of ['/', '/gateway', '/system', '/design-system', '/does-not-exist']) {
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
    for (const path of ['/', '/gateway', '/system', '/design-system']) {
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
