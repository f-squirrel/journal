import { test, expect, type Page } from "./coverage";

// Corners of the entry pane (`pages/EntryPane.tsx`) the other specs pass by:
// a route naming nothing, dismissing each overlay, and the toolbar's own
// image button. All against the fallback.

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

function editor(page: Page) {
  return page.getByTestId("entry-body").locator(".cm-content");
}

async function newEntry(page: Page, title: string, body = "") {
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill(title);
  if (body) await editor(page).fill(body);
}

test("a route naming no entry says it was not found", async ({ page }) => {
  await login(page);
  await page.evaluate(() => {
    window.location.hash = "#/entries/00000000-0000-4000-8000-000000000000";
  });
  await expect(page.getByText("Entry not found.")).toBeVisible();
});

test("the toolbar's image button opens the file chooser and inserts the image", async ({
  page,
}) => {
  await login(page);
  await newEntry(page, "Button", "text");

  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Insert image" }).click();
  await (
    await chooser
  ).setFiles({ name: "a.png", mimeType: "image/png", buffer: PNG });

  await expect(editor(page)).toContainText("](media:");
});

test("an image error can be dismissed", async ({ page }) => {
  await login(page);
  await newEntry(page, "Refused", "text");
  await page.getByTestId("image-input").setInputFiles({
    name: "a.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("just text"),
  });
  await expect(page.getByTestId("media-error")).toBeVisible();

  await page.getByTestId("media-error").getByRole("button").click();
  await expect(page.getByTestId("media-error")).toHaveCount(0);
});

test("the date picker closes on Escape without changing the date", async ({
  page,
}) => {
  await login(page);
  await page.getByText("Project ideas").click();
  const before = await page.getByTestId("entry-heading").innerText();

  await page.getByRole("button", { name: /Change entry date/ }).click();
  await expect(page.getByRole("gridcell").first()).toBeVisible();
  await page.keyboard.press("Escape");

  await expect(page.getByRole("gridcell")).toHaveCount(0);
  await expect(page.getByTestId("entry-heading")).toHaveText(before);
});

test("the delete confirmation closes on Escape and keeps the entry", async ({
  page,
}) => {
  await login(page);
  await page.getByText("Project ideas").click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Delete entry" }).click();
  await expect(
    page.getByRole("dialog", { name: "Delete entry?" }),
  ).toBeVisible();

  await page.keyboard.press("Escape");

  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(
    page.getByTestId("entry-row").filter({ hasText: "Project ideas" }),
  ).toBeVisible();
});

test("the notice about a kept copy can be dismissed", async ({ page }) => {
  await page.clock.install();
  await login(page, "?sync=pull");
  await page
    .getByTestId("entry-row")
    .filter({ hasText: "First day back" })
    .click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await editor(page).fill("mine");

  // A sync pass rewrites the entry under the open pane; the debounced save
  // then meets Stale and the draft becomes a copy.
  await page.evaluate(() => window.__syncTestHooks?.syncNow());
  await page.clock.runFor(1000);

  const notice = page.getByText("was kept as", { exact: false });
  await expect(notice).toBeVisible();
  await notice
    .locator("xpath=ancestor::*[@role='alert']")
    .getByRole("button", { name: "Close" })
    .click();
  await expect(notice).toHaveCount(0);
});
