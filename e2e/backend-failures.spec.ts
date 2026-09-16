import { test, expect, type Page } from "@playwright/test";

// Backend failures are invisible to the browser fallback by construction, so
// these tests inject them from the URL (see the fault-injection block in
// `src/api/entries.ts`). The app uses HashRouter, so the query string goes
// before the hash.

async function login(page: Page, query: string) {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "Entries" })).toBeVisible();
}

test("a failed save keeps the user in the editor", async ({ page }) => {
  await login(page, "?fail=save&failMessage=Disk+is+full.");

  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Entry title").fill("E2E test entry");
  await page.getByPlaceholder("Write in Markdown...").fill("# Hello from Playwright");
  await page.getByRole("button", { name: "Save" }).click();

  await expect(page.getByText("Disk is full.")).toBeVisible();
  expect(page.url()).toContain("#/entries/new/edit");
  await expect(page.getByPlaceholder("Write in Markdown...")).toHaveValue("# Hello from Playwright");
});

test("a failed delete keeps the row and re-enables the dialog", async ({ page }) => {
  await login(page, "?fail=delete&failMessage=Disk+is+full.");

  await page.getByText("Project ideas").click();
  await page.getByRole("button", { name: "Delete" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();

  await expect(page.getByText("Disk is full.")).toBeVisible();
  await expect(page.getByRole("dialog").getByRole("button", { name: "Cancel" })).toBeEnabled();

  // The entry is still on disk, so it must still be in the list. Wait for the
  // dialog's close transition: its backdrop swallows the click on "Back".
  await page.getByRole("dialog").getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("dialog")).toBeHidden();
  await page.getByRole("button", { name: "Back" }).click();
  await expect(page.getByRole("heading", { name: "Entries" })).toBeVisible();
  await expect(page.getByText("Project ideas")).toBeVisible();
});

test("a failed load does not render stale content", async ({ page }) => {
  await login(page, "?fail=get&failMessage=Vault+is+locked.");

  await page.getByText("First day back").click();

  await expect(page.getByText("Vault is locked.")).toBeVisible();
  await expect(page.getByText("Started journaling again.")).toHaveCount(0);
});

test("an unlock failure shows the backend's own message", async ({ page }) => {
  await page.goto("/?fail=unlock&failMessage=Vault+key+file+is+missing.");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();

  await expect(page.getByText("Vault key file is missing.")).toBeVisible();
  // Load-bearing: a fix that appends the real error under the old constant
  // would otherwise pass, and "Incorrect passphrase." is the one failure the
  // app declares unrecoverable.
  await expect(page.getByText("Incorrect passphrase.")).toHaveCount(0);
  // P2-5: reachable only now that `login` rejects. Without the `finally` the
  // button stays disabled forever and the user is stranded on this screen.
  await expect(page.getByRole("button", { name: "Sign in" })).toBeEnabled();
});

// `?slow=list` holds `list_entries` for 1200 ms (SLOW_MS in `src/api/entries.ts`),
// which is what a real unlock-and-list looks like now that commands run off
// Tauri's main thread. The constant is well clear of Playwright's action time
// on purpose; don't tighten it.

test("the entry list renders nothing until the backend answers", async ({ page }) => {
  await login(page, "?slow=list");

  // Fabricated rows inside a real journal: `entries` must start empty rather
  // than from anything the frontend made up.
  await expect(page.getByText("First day back")).toHaveCount(0);
  await expect(page.getByText("First day back")).toBeVisible();
});

test("a list in flight at logout cannot repopulate the next session", async ({ page }) => {
  await login(page, "?slow=list");

  // Log out with the listing still in flight, then sit out the delay on the
  // login screen so the stale promise resolves while logged out.
  await page.getByRole("button", { name: "Log out" }).click();
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
  await page.waitForTimeout(2000);
  await expect(page.getByText("First day back")).toHaveCount(0);

  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "Entries" })).toBeVisible();
  // Without the cancellation guard the previous session's titles are already
  // back in state, so they render before this listing has answered.
  await expect(page.getByText("First day back")).toHaveCount(0);
});
