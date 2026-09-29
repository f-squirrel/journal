// The `test` every spec imports, so that `make coverage-web` can collect
// front-end coverage from the Playwright suite. Without `COVERAGE` set it is
// plain Playwright: nothing is patched and nothing is written.
//
// With it, the dev server instruments `src/` (vite.config.ts) and each page
// keeps its counters in `window.__coverage__`. Those die with the document, so
// they are saved just before `page.goto`/`page.reload` replace it, and from
// whatever is still open when the test ends. Every save is one file in
// `.nyc_output/`; `nyc report` merges them.
//
// Why wrap the two calls rather than listen for the page going away: a
// `pagehide` handler calling an exposed function never arrives (measured: a
// spec that navigated twice saved one file, not two), and a `framenavigated`
// listener runs after the old document is gone. A navigation the *page*
// starts (a link, `location.reload()`) would therefore be missed, but the app
// starts none (it is a single page under `HashRouter`, and a hash change keeps
// the document), and the specs navigate only through those two calls.
//
// Specs import `test`, `expect` and `Page` from here rather than from
// `@playwright/test`, which is the whole of their change.

import { randomUUID } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import process from "node:process";
import { test as base, type Page } from "@playwright/test";

export { expect } from "@playwright/test";
export type { Page } from "@playwright/test";

declare global {
  interface Window {
    /// Only the dev fallback installs this (`src/api/fallback.ts`).
    __syncTestHooks?: { syncNow(): Promise<void> };
  }
}

const enabled = !!process.env.COVERAGE;
const outDir = ".nyc_output";

/// Save the counters of the document `page` is showing now, if it has any.
async function collect(page: Page) {
  const json = await page
    .evaluate(() => {
      const c = (window as unknown as { __coverage__?: unknown }).__coverage__;
      return c ? JSON.stringify(c) : null;
    })
    // A page that is closed, crashed or still on `about:blank` has nothing.
    .catch(() => null);
  if (!json) return;
  mkdirSync(outDir, { recursive: true });
  writeFileSync(`${outDir}/${process.pid}-${randomUUID()}.json`, json);
}

export const test = base.extend({
  page: async ({ page, context }, use) => {
    if (!enabled) {
      await use(page);
      return;
    }

    const goto = page.goto.bind(page);
    page.goto = async (...args) => {
      await collect(page);
      return goto(...args);
    };
    const reload = page.reload.bind(page);
    page.reload = async (...args) => {
      await collect(page);
      return reload(...args);
    };

    await use(page);

    for (const open of context.pages()) await collect(open);
  },
});
