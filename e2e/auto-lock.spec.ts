import { test, expect, type Page } from "@playwright/test";

// The idle lock (`useIdleLock`, wired up in `AppStateProvider`) runs on a
// 10 s timer, so these tests drive it with Playwright's fake clock rather than
// waiting. `install` goes before `goto` so the page's timers are the fake ones
// from the first script.
//
// `runFor` fires every timer that falls inside the span, in order, which is
// what a chain of debounce -> write -> lock needs. `fastForward` jumps and
// fires each due timer once, the way a webview resumes after being suspended.

const IDLE_MS = 10_000;

async function login(page: Page, query = "") {
  await page.clock.install();
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
  // Rows are the proof that the listing landed, i.e. the session is fully up.
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
}

function locked(page: Page) {
  return expect(page.getByLabel("Passphrase")).toBeVisible();
}

function unlocked(page: Page) {
  return expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

async function signIn(page: Page) {
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await unlocked(page);
}

/// The entry body, rendered or in the editor — same test id either way.
function editor(page: Page) {
  return page.getByTestId("entry-body").locator(".cm-content");
}

async function newDraft(page: Page, title: string, body: string) {
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill(title);
  await editor(page).fill(body);
}

test("locks after ten idle seconds and clears the decrypted titles", async ({
  page,
}) => {
  await login(page);
  await expect(page.getByText("First day back")).toBeVisible();

  await page.clock.runFor(IDLE_MS);

  await locked(page);
  await expect(page.getByText("First day back")).toHaveCount(0);
  await expect(page.getByText("Project ideas")).toHaveCount(0);
});

test("does not lock a moment before the deadline", async ({ page }) => {
  await login(page);

  await page.clock.runFor(IDLE_MS - 1_000);

  await unlocked(page);
  await expect(page.getByText("First day back")).toBeVisible();
});

test("pointer activity restarts the countdown", async ({ page }) => {
  await login(page);

  await page.clock.runFor(6_000);
  await page.mouse.move(300, 300);
  await page.clock.runFor(6_000);
  // Twelve seconds since login, but only six since the last movement.
  await unlocked(page);

  await page.clock.runFor(5_000);
  await locked(page);
});

test("typing counts as activity", async ({ page }) => {
  await login(page);
  await page.getByText("First day back").click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await editor(page).click();

  await page.clock.runFor(6_000);
  await page.keyboard.press("x");
  await page.clock.runFor(6_000);
  await unlocked(page);
  await expect(editor(page)).toContainText("x");

  await page.clock.runFor(5_000);
  await locked(page);
});

/// Search is the one place the user types without an entry open, so a search
/// field the idle timer did not watch would lock the vault mid-query.
test("typing in the search field counts as activity", async ({ page }) => {
  await login(page);
  await page.getByLabel("Search entries").click();

  await page.clock.runFor(6_000);
  await page.keyboard.type("recipe");
  await page.clock.runFor(6_000);
  // Twelve seconds since login, but only six since the last keystroke — and
  // the search behind them ran.
  await unlocked(page);
  await expect(page.getByTestId("entry-row")).toHaveCount(1);

  await page.clock.runFor(5_000);
  await locked(page);
});

test("an edit made before the lock is there after unlocking", async ({
  page,
}) => {
  await login(page);
  await newDraft(page, "Written before the lock", "It must survive.");

  // The debounce (800 ms) writes it, then the lock lands.
  await page.clock.runFor(IDLE_MS);
  await locked(page);

  await signIn(page);
  await page
    .getByTestId("entry-row")
    .filter({ hasText: "Written before the lock" })
    .click();
  await expect(page.getByTestId("entry-body")).toContainText(
    "It must survive.",
  );
});

test("a write still in flight when the timer fires is not lost", async ({
  page,
}) => {
  await login(page, "?slow=save");
  // Stop time so the write cannot finish on its own while the lock waits.
  await page.clock.pauseAt(Date.now() + 1_000);
  await newDraft(page, "Slow to write", "Still in flight.");

  // Both the debounce and the idle timer are due after the jump: the write
  // starts, then the idle lock finds it running.
  await page.clock.fastForward(IDLE_MS);
  await expect(page.getByTestId("save-status")).toHaveText("Saving…");
  await expect(page.getByLabel("Passphrase")).toHaveCount(0);

  // Let the write finish; only then may the vault lock.
  await page.clock.runFor(2_000);
  await locked(page);

  await page.clock.resume();
  await signIn(page);
  await expect(
    page.getByTestId("entry-row").filter({ hasText: "Slow to write" }),
  ).toBeVisible();
});

test("a failed save keeps the journal unlocked, and log out still works", async ({
  page,
}) => {
  await login(page, "?fail=save&failMessage=Disk+is+full.");
  await newDraft(page, "Cannot be saved", "Only in memory.");

  await page.clock.runFor(1_000);
  await expect(page.getByText("Disk is full.")).toBeVisible();

  // Several idle periods: each retries the write, fails, and stays open.
  await page.clock.runFor(IDLE_MS * 3);
  await expect(page.getByLabel("Passphrase")).toHaveCount(0);
  await expect(page.getByText("Disk is full.")).toBeVisible();
  await expect(editor(page)).toContainText("Only in memory.");

  // The user asking to leave is not blocked by a broken backend.
  await page.getByRole("button", { name: "Log out" }).click();
  await locked(page);
});

test("manual log out flushes a pending draft first", async ({ page }) => {
  await login(page);
  await newDraft(page, "Typed then left", "Logged out mid-debounce.");

  // No time passes: the 800 ms debounce is still pending when Log out lands.
  await page.getByRole("button", { name: "Log out" }).click();
  await locked(page);

  await signIn(page);
  await expect(
    page.getByTestId("entry-row").filter({ hasText: "Typed then left" }),
  ).toBeVisible();
});
