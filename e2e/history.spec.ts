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
    historyDialog(page).getByRole("alert").filter({ hasText: "history broke" }),
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

test("restoring right after typing, with the save still pending, is not refused", async ({
  page,
}) => {
  await login(page);
  await page.getByText("First day back").click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  // No wait for "Saved": opening history flushes this edit itself, which
  // moves the live entry's `updated_at` past what the pane loaded.
  await editor(page).fill("Typed just before opening history");

  await openHistory(page);
  const dialog = historyDialog(page);
  await dialog.getByText("First day back").click();
  await expect(dialog.getByTestId("history-preview")).toContainText(
    "Started journaling again",
  );
  await dialog.getByRole("button", { name: "Restore this version" }).click();

  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("entry-body")).toContainText(
    "Started journaling again",
  );
});

test("a second restore in the same pane is not refused", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("Twice restored");
  await editor(page).fill("Version one");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  await editor(page).fill("Version two");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");

  const dialog = historyDialog(page);
  for (const expected of ["Version one", "Version two"]) {
    await openHistory(page);
    // Newest first: after the first restore, "Version two" is the newest.
    await dialog
      .getByRole("button")
      .filter({ hasText: "Twice restored" })
      .first()
      .click();
    await expect(dialog.getByTestId("history-preview")).toContainText(expected);
    await dialog.getByRole("button", { name: "Restore this version" }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByTestId("entry-body")).toContainText(expected);
  }
});

test("a preview that arrives late for a version no longer selected is not shown", async ({
  page,
}) => {
  // Only previews are slow (`version`), so the saves below are not.
  await login(page, "?slow=version&slowMs=1000");
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("Out of order");
  for (const body of ["Older body", "Newer body", "Current body"]) {
    await editor(page).fill(body);
    await expect(page.getByTestId("save-status")).toHaveText("Saved");
  }

  await page.clock.install();

  await openHistory(page);
  const dialog = historyDialog(page);
  const rows = dialog.getByRole("button").filter({ hasText: "Out of order" });
  await expect(rows).toHaveCount(2);
  // Pick the older version, then the newer one before the first arrives.
  await rows.nth(1).click();
  await page.clock.runFor(500);
  await rows.nth(0).click();
  // The first preview has arrived; the second has not.
  await page.clock.runFor(600);
  await expect(dialog.getByTestId("history-preview")).not.toContainText(
    "Older body",
  );
  await expect(
    dialog.getByRole("button", { name: "Restore this version" }),
  ).toBeDisabled();

  await page.clock.runFor(500);
  await expect(dialog.getByTestId("history-preview")).toContainText(
    "Newer body",
  );
});
