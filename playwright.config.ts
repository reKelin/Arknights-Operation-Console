import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/smoke/ui",
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: 1,
  maxFailures: process.env.CI ? 1 : undefined,
  // 单个交互卡死的保护，不是整条 CI 的性能门槛。
  timeout: 30_000,
  expect: { timeout: 5_000 },
  outputDir: ".local/smoke-results",
  reporter: [
    ["list"],
    ["html", { outputFolder: ".local/smoke-report", open: "never" }],
  ],
  use: {
    browserName: "chromium",
    baseURL: "http://127.0.0.1:1420",
    viewport: { width: 1100, height: 280 },
    actionTimeout: 5_000,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "off",
  },
  webServer: {
    command: "npm run dev -- --host 127.0.0.1",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
