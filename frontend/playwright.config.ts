import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright projects
 *
 *   ui      Mocked API (deterministic). Shell, navigation, keyboard, states,
 *           responsive overflow, axe accessibility, console errors.
 *   visual  Mocked API + frozen clock. Screenshot comparison against
 *           approved baselines. Run ONLY in the pinned Playwright container
 *           (scripts/playwright.ps1 / CI) so font rendering is reproducible.
 *   e2e     Real stack (no mocks). Requires E2E_BASE_URL, e.g.
 *           http://127.0.0.1:3000 (Compose) or http://127.0.0.1:5173 (Vite).
 *
 * ui and visual run against a `--mode showcase` production build served by
 * `vite preview`, so the internal /design-system route exists.
 */
const showcasePort = 4173;
const e2eBaseUrl = process.env.E2E_BASE_URL;

export default defineConfig({
  outputDir: 'test-results',
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  snapshotPathTemplate: 'tests/visual/__screenshots__/{testFileName}/{arg}{ext}',
  expect: {
    toHaveScreenshot: {
      animations: 'disabled',
      caret: 'hide',
      scale: 'css',
      // Small tolerance for sub-pixel anti-aliasing only; layout changes fail.
      maxDiffPixelRatio: 0.002,
    },
  },
  use: {
    ...devices['Desktop Chrome'],
    colorScheme: 'dark',
    locale: 'en-US',
    timezoneId: 'UTC',
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'ui',
      testDir: 'tests/ui',
      use: { baseURL: `http://127.0.0.1:${showcasePort.toString()}` },
    },
    {
      name: 'visual',
      testDir: 'tests/visual',
      use: { baseURL: `http://127.0.0.1:${showcasePort.toString()}` },
    },
    {
      name: 'e2e',
      testDir: 'tests/e2e',
      use: { baseURL: e2eBaseUrl ?? 'http://127.0.0.1:3000' },
    },
  ],
  // e2e targets an already running stack; only ui/visual need the showcase build.
  webServer: e2eBaseUrl
    ? undefined
    : {
        command: 'npm run build:showcase && npm run preview:showcase -- --host 127.0.0.1',
        url: `http://127.0.0.1:${showcasePort.toString()}`,
        reuseExistingServer: !process.env.CI,
        timeout: 180_000,
        stdout: 'ignore',
        stderr: 'pipe',
      },
});
