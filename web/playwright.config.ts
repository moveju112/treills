import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  use: { baseURL: 'http://127.0.0.1:4175', headless: true, browserName: 'chromium' },
  webServer: { command: 'npm run dev -- --port 4175', url: 'http://127.0.0.1:4175', reuseExistingServer: !process.env.CI, timeout: 60_000 },
});
