import { test, expect, type Page } from "@playwright/test";

// These tests run against the Vite dev server without Tauri's `invoke` bridge,
// so the app falls back to its seeded in-memory entries (see
// context/AppStateContext.tsx). They exercise frontend logic and navigation,
// not the Rust storage backend.

async function login(page: Page) {
  await page.goto("/");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "Entries" })).toBeVisible();
}

test("lists entry headers without their markdown body", async ({ page }) => {
  await login(page);
  await expect(page.getByText("First day back")).toBeVisible();
  await expect(page.getByText("Project ideas")).toBeVisible();
  await expect(page.getByText("Started journaling again")).toHaveCount(0);
});

test("opens an entry and loads its full content on demand", async ({ page }) => {
  await login(page);
  await page.getByText("First day back").click();
  await expect(page.getByRole("heading", { name: "First day back", level: 4 })).toBeVisible();
  await expect(page.getByText("Started journaling again.")).toBeVisible();
});

test("editing an entry loads its existing markdown", async ({ page }) => {
  await login(page);
  await page.getByText("First day back").click();
  await page.getByRole("button", { name: "Edit" }).click();
  await expect(page.locator("textarea").first()).toHaveValue(/Started journaling again/);
});

test("creating a new entry adds it to the list", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Entry title").fill("E2E test entry");
  await page.getByPlaceholder("Write in Markdown...").fill("# Hello from Playwright");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("heading", { name: "E2E test entry" })).toBeVisible();

  await page.getByRole("button", { name: "Entries" }).click();
  await expect(page.getByText("E2E test entry")).toBeVisible();
});

test("deleting an entry removes it from the list", async ({ page }) => {
  await login(page);
  await page.getByText("Project ideas").click();
  await page.getByRole("button", { name: "Delete" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
  await expect(page.getByRole("heading", { name: "Entries" })).toBeVisible();
  await expect(page.getByText("Project ideas")).toHaveCount(0);
});
