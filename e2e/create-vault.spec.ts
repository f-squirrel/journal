import { test, expect, type Page } from "@playwright/test";

// The create-vault form is only reachable when there is no vault yet, which
// never happens under the browser fallback on its own — so every case here
// goes through `?vault=new` (see the fault-injection block in
// `src/api/entries.ts`). The app uses HashRouter, so the query string goes
// before the hash.

/// The minimum, read off the field's own helper text rather than duplicated
/// here: the number is the app's to state, and a copy of it in the suite is a
/// second source of truth that can drift.
async function minimumLength(page: Page): Promise<number> {
  const helper = await page.getByText(/at least \d+ characters/i).innerText();
  const match = helper.match(/\d+/);
  expect(match, `no minimum stated in "${helper}"`).not.toBeNull();
  return Number(match![0]);
}

test("the create-vault form asks for confirmation", async ({ page }) => {
  await page.goto("/?vault=new");

  await expect(page.getByLabel("Passphrase", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Confirm passphrase")).toBeVisible();
  await expect(page.getByRole("button", { name: "Create vault" })).toBeVisible();
});

test("a mismatched confirmation blocks submit", async ({ page }) => {
  await page.goto("/?vault=new");

  await page.getByLabel("Passphrase", { exact: true }).fill("correct horse battery");
  await page.getByLabel("Confirm passphrase").fill("correct horse bettery");
  await page.getByRole("button", { name: "Create vault" }).click();

  await expect(page.getByText("The two passphrases don't match.")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Entries" })).toHaveCount(0);
});

test("a too-short passphrase blocks submit", async ({ page }) => {
  await page.goto("/?vault=new");

  const short = "x".repeat((await minimumLength(page)) - 1);
  await page.getByLabel("Passphrase", { exact: true }).fill(short);
  await page.getByLabel("Confirm passphrase").fill(short);
  await page.getByRole("button", { name: "Create vault" }).click();

  await expect(page.getByRole("alert").filter({ hasText: /at least \d+ characters/i })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Entries" })).toHaveCount(0);
});

test("a matching pair creates the vault", async ({ page }) => {
  await page.goto("/?vault=new");

  const long = "x".repeat(await minimumLength(page));
  await page.getByLabel("Passphrase", { exact: true }).fill(long);
  await page.getByLabel("Confirm passphrase").fill(long);
  await page.getByRole("button", { name: "Create vault" }).click();

  await expect(page.getByRole("heading", { name: "Entries" })).toBeVisible();
});
