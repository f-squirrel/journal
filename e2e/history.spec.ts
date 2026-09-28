import { test, expect, type Page } from "./coverage";

// Version history, entirely against the fallback (`src/api/fallback.ts`),
// which pushes an entry's previous content into its history on every save —
// unlike the real backend's 30-minute throttle — so history exists after just
// two edits. See the fallback's `pushHistory` for why.

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

function historyDialog(page: Page) {
  return page.getByRole("dialog", { name: "Version history" });
}

async function openHistory(page: Page) {
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Version history" }).click();
}

test("lists a past version, previews it, and restores it", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("History test entry");
  await editor(page).fill("First body");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");

  await editor(page).fill("Second body");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");

  await openHistory(page);
  const dialog = historyDialog(page);
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("History test entry")).toBeVisible();

  // Selecting the listed version previews the *first* body, not the second.
  await dialog.getByText("History test entry").click();
  await expect(dialog.getByTestId("history-preview")).toContainText(
    "First body",
  );
  await expect(dialog.getByTestId("history-preview")).not.toContainText(
    "Second body",
  );

  await dialog.getByRole("button", { name: "Restore this version" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("entry-body")).toContainText("First body");
  await expect(
    page.getByTestId("entry-row").filter({ hasText: "History test entry" }),
  ).toBeVisible();
});

test("an entry with only one save shows the empty state", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("Only one save");
  await editor(page).fill("Nothing to go back to yet");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");

  await openHistory(page);
  await expect(
    historyDialog(page).getByText("No earlier versions of this entry yet."),
  ).toBeVisible();
});

test("the menu item is disabled on an unsaved new draft", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await expect(
    page.getByRole("menuitem", { name: "Version history" }),
  ).toBeDisabled();
});

test("a failed listing shows the backend's error", async ({ page }) => {
  await login(page, "?fail=history&failMessage=history%20broke");
  await page.getByText("First day back").click();
  await openHistory(page);

  await expect(
    historyDialog(page)
      .getByRole("alert")
      .filter({ hasText: "history broke" }),
  ).toBeVisible();
});

test("a failed restore shows the backend's error and leaves the dialog open", async ({
  page,
}) => {
  await login(page, "?fail=restore&failMessage=restore%20broke");
  await page.getByText("First day back").click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await editor(page).fill("Edited so there is a version to restore");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");

  await openHistory(page);
  const dialog = historyDialog(page);
  await dialog.getByText("First day back").click();
  await expect(dialog.getByTestId("history-preview")).toContainText(
    "Started journaling again",
  );

  await dialog.getByRole("button", { name: "Restore this version" }).click();

  await expect(
    dialog.getByRole("alert").filter({ hasText: "restore broke" }),
  ).toBeVisible();
  await expect(dialog).toBeVisible();
});

test("opening history flushes a pending save first, and skips opening if it's still unsaved", async ({
  page,
}) => {
  // Every save fails, so the edit below never settles: `openHistory`'s flush
  // leaves the draft dirty, and the guard must not open a restore dialog
  // against a base it can no longer be sure of.
  await login(page, "?fail=save&failMessage=save%20broke");
  await page.getByText("First day back").click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await editor(page).fill("An edit that will never save");
  await expect(page.getByTestId("save-status")).toHaveText("Couldn't save");

  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Version history" }).click();

  await expect(historyDialog(page)).toBeHidden();
  // The failed save's own error is still on the pane, untouched.
  await expect(page.getByTestId("save-status")).toHaveText("Couldn't save");
});
