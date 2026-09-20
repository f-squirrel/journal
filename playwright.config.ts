import { defineConfig, devices } from "@playwright/test";

const port = 5183;

// `make coverage-web` sets this: the dev server then instruments `src/`, and
// `e2e/coverage.ts` collects the counters. A server left running from an
// ordinary run is not instrumented, so coverage never reuses one.
const coverage = !!process.env.COVERAGE;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  reporter: "list",
  use: {
    baseURL: `http://localhost:${port}`,
    trace: "on-first-retry",
  },
  webServer: {
    command: `npm run dev -- --port ${port} --strictPort`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI && !coverage,
    env: coverage ? { VITE_COVERAGE: "true" } : {},
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
