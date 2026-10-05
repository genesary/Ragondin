import { defineConfig, devices } from '@playwright/test';

// The end-to-end journeys (ARCHITECTURE.md § The end-to-end journeys): each
// test starts the real binary over its own copy of the fixture workspace, so
// tests share nothing and run in parallel. `just test-ui-e2e` builds both and
// names them in RAGONDIN_E2E_BINARY and RAGONDIN_E2E_FIXTURE.
export default defineConfig({
  testDir: 'e2e',
  testMatch: '**/*.spec.ts',
  fullyParallel: true,
  forbidOnly: process.env.CI !== undefined,
  retries: 0,
  // A journey launches runs and waits for them: a debug binary embeds the
  // toy models' corpus in well under a second, the rest is the browser.
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: process.env.CI === undefined ? 'list' : [['list'], ['github']],
  // Traces of failed tests, under the workspace's build directory, which git
  // already ignores.
  outputDir: '../target/ui-e2e',
  use: { trace: 'retain-on-failure' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
