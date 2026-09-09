import { test, expect, type Page } from "@playwright/test";

// Backend failures are invisible to the browser fallback by construction, so
// these tests inject them from the URL (see the fault-injection block in
// `src/api/entries.ts`). The app uses HashRouter, so the query string goes
// before the hash.

async function login(page: Page, query: string) {
  await page.goto(`/${query}`);
  await page.getByLabel("Email").fill("test@example.com");
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
