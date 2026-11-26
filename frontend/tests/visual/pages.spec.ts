import { expect, test } from '@playwright/test';

import { mockBackend, type Scenario, settle, VIEWPORTS } from '../support/mockApi.ts';

/**
 * Visual regression. Baselines live in tests/visual/__screenshots__ and are
 * CANDIDATES until the owner approves them (see docs/development/TESTING.md).
 * Run only inside the pinned Playwright container: scripts/playwright.ps1.
 */
const pages: { name: string; path: string; scenario: Scenario; fullPage: boolean }[] = [
  { name: 'overview', path: '/', scenario: 'healthy', fullPage: true },
  { name: 'system', path: '/system', scenario: 'healthy', fullPage: true },
  { name: 'design-system', path: '/design-system', scenario: 'healthy', fullPage: true },
  { name: 'overview-offline', path: '/', scenario: 'offline', fullPage: false },
  { name: 'system-database-down', path: '/system', scenario: 'database-down', fullPage: false },
];

for (const vp of VIEWPORTS) {
  test.describe(vp.name, () => {
    test.use({ viewport: { width: vp.width, height: vp.height } });

    for (const p of pages) {
      // State variants only at one desktop and the mobile width.
      if (p.scenario !== 'healthy' && vp.width !== 1440 && vp.width !== 390) continue;

      test(p.name, async ({ page }) => {
        await mockBackend(page, p.scenario);
        await page.goto(p.path);
        if (p.scenario === 'offline') {
          await expect(page.getByTestId('mode-indicator')).toHaveAttribute(
            'data-mode',
            'unverified',
          );
        }
        await settle(page, { allowBusy: p.path === '/design-system' || p.scenario === 'offline' });
        await expect(page).toHaveScreenshot(`${p.name}-${vp.name}.png`, { fullPage: p.fullPage });
      });
    }
  });
}
