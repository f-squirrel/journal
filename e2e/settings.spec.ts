import { test, expect, type Page } from "./coverage";

// WP5: the Settings page (`src/pages/SettingsPage.tsx`) and the effects of
// the journal settings it exposes — `formatTime`'s `TimeFormat`, the
// calendar's week start, and the entry template's autosave guard. WP4 already
// wired colour mode, shortcuts and the idle lock onto real settings and
// covered the plumbing in `settings-store.spec.ts`; this file drives the
// controls those tests could only reach through a debug hook.

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

async function seedDeviceSettings(page: Page, device: Record<string, unknown>) {
  await page.addInitScript((json) => {
    window.localStorage.setItem("journal.fallback.device", json);
  }, JSON.stringify(device));
}

function settingsPage(page: Page) {
  return page.getByTestId("settings-page");
}

async function openSettings(page: Page) {
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(settingsPage(page)).toBeVisible();
}

/// A `TextField select`'s combobox, found by its label the way a real user
/// would, then the option picked from the popover it opens.
async function selectValue(page: Page, label: string, optionName: string) {
  await settingsPage(page).getByLabel(label).click();
  await page.getByRole("option", { name: optionName }).click();
}

function templateField(page: Page) {
  return settingsPage(page).getByTestId("entry-template");
}

/// Types into the template field and blurs it — the path that saves —
/// leaving the caller to wait for the value to show wherever it checks next.
async function fillTemplate(page: Page, text: string) {
  const field = templateField(page);
  await field.fill(text);
  await field.evaluate((el) => (el as HTMLElement).blur());
}

/// The entry pane, whichever mode it's in — same test id either way.
function editor(page: Page) {
  return page.getByTestId("entry-body").locator(".cm-content");
}

function noOverflow(page: Page) {
  return page.evaluate(
    () =>
      document.documentElement.scrollWidth <=
      document.documentElement.clientWidth,
  );
}

test("opens from the rail and from $mod+Comma", async ({ page }) => {
  await login(page);

  await openSettings(page);
  await page.goBack();
  await expect(settingsPage(page)).toHaveCount(0);

  await page.keyboard.press("ControlOrMeta+,");
  await expect(settingsPage(page)).toBeVisible();
});

test("a device setting persists across a reload", async ({ page }) => {
  await login(page);
  await openSettings(page);

  await selectValue(page, "Lock when idle", "Never");
  await expect(settingsPage(page).getByLabel("Lock when idle")).toHaveText(
    "Never",
  );

  await page.reload();
  await login(page);
  await openSettings(page);
  await expect(settingsPage(page).getByLabel("Lock when idle")).toHaveText(
    "Never",
  );
});

test("a journal setting persists across a lock and unlock", async ({
  page,
}) => {
  await login(page);
  await openSettings(page);

  await selectValue(page, "Week starts on", "Monday");
  await expect(settingsPage(page).getByLabel("Week starts on")).toHaveText(
    "Monday",
  );

  await page.getByRole("button", { name: "Log out" }).click();
  await expect(page.getByLabel("Passphrase")).toBeVisible();
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();

  await openSettings(page);
  await expect(settingsPage(page).getByLabel("Week starts on")).toHaveText(
    "Monday",
  );
});

test.describe("time format", () => {
  // Pinned so the default ("Automatic") reads predictably before either
  // explicit format is chosen — see `formatTime` in `src/utils/date.ts`.
  test.use({ timezoneId: "UTC", locale: "en-US" });

  test("h24 and h12 change how a row's time reads", async ({ page }) => {
    await login(page);
    // Seeded at 2026-09-02T21:05:00.000Z (`src/api/fallback.ts`).
    const row = page
      .getByTestId("entry-row")
      .filter({ hasText: "Project ideas" });

    await openSettings(page);
    await selectValue(page, "Time format", "24-hour");
    await page.goBack();
    await expect(row).toContainText("21:05");

    await openSettings(page);
    await selectValue(page, "Time format", "12-hour");
    await page.goBack();
    await expect(row).toContainText("9:05 PM");
  });
});

test("Monday week start puts Monday first in the calendar", async ({
  page,
}) => {
  await login(page);
  await openSettings(page);
  await selectValue(page, "Week starts on", "Monday");
  await page.goBack();

  await page.getByRole("button", { name: /^Calendar/ }).click();
  await expect(page.getByRole("heading", { name: "Calendar" })).toBeVisible();
  await expect(page.getByRole("columnheader").first()).toHaveAccessibleName(
    "Monday",
  );
});

/// Opens the other seeded entry and waits for its own text to be on screen —
/// not just for the row count, which a late wrongful write could still leave
/// at 2 the instant this runs, before the debounced save lands. Waiting for
/// the pane first gives that write time to happen, so the row-count check
/// after it actually catches one.
async function openProjectIdeas(page: Page) {
  await page.getByText("Project ideas").click();
  await expect(page.getByTestId("entry-body")).toContainText(
    "Some things I want to build",
  );
}

test("a template pre-fills a new draft, and leaving without typing writes no entry", async ({
  page,
}) => {
  await login(page);
  await openSettings(page);
  await fillTemplate(page, "## Mood\n\n## Gratitude");
  await page.goBack();

  await page.getByRole("button", { name: "New entry" }).first().click();
  await expect(editor(page)).toContainText("Mood");

  // Leaving by opening another entry, without typing: nothing calls
  // `noteChange`, so `dirty` never becomes true and nothing is written.
  await openProjectIdeas(page);
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
});

test("typing into a templated draft and deleting back to the template, then leaving, writes no entry", async ({
  page,
}) => {
  await page.clock.install();
  await login(page);
  await openSettings(page);
  await fillTemplate(page, "## Mood");
  await page.goBack();

  await page.getByRole("button", { name: "New entry" }).first().click();
  await expect(editor(page)).toContainText("Mood");
  await editor(page).fill("## Mood\n\nSomething typed, then removed.");
  await editor(page).fill("## Mood");

  // Long enough for the 800 ms debounce to have fired and, if the guard were
  // wrong, to show "Saved".
  await page.clock.runFor(1_200);
  await expect(page.getByTestId("save-status")).not.toHaveText("Saved");

  await openProjectIdeas(page);
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
});

test("clearing a templated draft's body completely, then leaving, writes no entry", async ({
  page,
}) => {
  await page.clock.install();
  await login(page);
  await openSettings(page);
  await fillTemplate(page, "## Mood");
  await page.goBack();

  await page.getByRole("button", { name: "New entry" }).first().click();
  await expect(editor(page)).toContainText("Mood");
  // Deleted past the template, to nothing at all — not just back to it.
  await editor(page).fill("");

  await page.clock.runFor(1_200);
  await expect(page.getByTestId("save-status")).not.toHaveText("Saved");

  await openProjectIdeas(page);
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
});

test("'Never' in the Settings page disables the idle lock", async ({
  page,
}) => {
  await page.clock.install();
  await login(page);
  await openSettings(page);
  await selectValue(page, "Lock when idle", "Never");
  await page.goBack();

  await page.clock.runFor(60 * 60_000);
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
});

test("a template typed and left focused survives navigating to another route", async ({
  page,
}) => {
  await login(page);
  await openSettings(page);
  const text = "Left focused, not blurred";
  await templateField(page).fill(text);

  // Navigated by a shortcut, not a click elsewhere: React does not fire
  // `blur` for a focused element that is simply removed, which is the gap
  // the unmount save closes.
  await page.keyboard.press("ControlOrMeta+Shift+c");
  await expect(page.getByRole("heading", { name: "Calendar" })).toBeVisible();

  await openSettings(page);
  await expect(templateField(page)).toHaveValue(text);
});

test("a template typed and left focused survives an idle lock", async ({
  page,
}) => {
  await seedDeviceSettings(page, { idle_lock_minutes: 1 });
  await page.clock.install();
  await login(page);
  await openSettings(page);
  const text = "Saved by the idle lock's flush";
  await templateField(page).fill(text);

  await page.clock.runFor(60_000);
  await expect(page.getByLabel("Passphrase")).toBeVisible();

  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();

  await openSettings(page);
  await expect(templateField(page)).toHaveValue(text);
});

test("the page works at phone width", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page);

  await page.getByRole("button", { name: "Open menu" }).click();
  await openSettings(page);
  await expect(page.getByTestId("entry-list")).toHaveCount(0);
  expect(await noOverflow(page)).toBe(true);

  await page.getByRole("button", { name: "Back" }).click();
  await expect(page.getByTestId("entry-list")).toBeVisible();
});

test("colour mode persists across a reload", async ({ page }) => {
  await login(page);
  await openSettings(page);
  const group = settingsPage(page).getByRole("group", {
    name: "Colour mode",
  });
  await group.getByRole("button", { name: "Dark" }).click();
  await expect(group.getByRole("button", { name: "Dark" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  await page.reload();
  await login(page);
  await openSettings(page);
  await expect(
    settingsPage(page)
      .getByRole("group", { name: "Colour mode" })
      .getByRole("button", { name: "Dark" }),
  ).toHaveAttribute("aria-pressed", "true");
});

test("time format persists across a lock and unlock", async ({ page }) => {
  // A journal setting, like `week_starts_on` above: the fallback keeps it
  // only in memory, so it survives a lock and unlock but not a page reload
  // (unlike the device half, which persists to `localStorage`).
  await login(page);
  await openSettings(page);
  await selectValue(page, "Time format", "24-hour");
  await expect(settingsPage(page).getByLabel("Time format")).toHaveText(
    "24-hour",
  );

  await page.getByRole("button", { name: "Log out" }).click();
  await expect(page.getByLabel("Passphrase")).toBeVisible();
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();

  await openSettings(page);
  await expect(settingsPage(page).getByLabel("Time format")).toHaveText(
    "24-hour",
  );
});

test("the sync check is disabled with a hint when no provider is connected", async ({
  page,
}) => {
  await login(page);
  await openSettings(page);
  await expect(
    settingsPage(page).getByLabel("Check for sync every"),
  ).toBeDisabled();
  await expect(
    settingsPage(page).getByText("Connect a cloud account to change this."),
  ).toBeVisible();
});

test("the sync check persists across a reload once a provider is connected", async ({
  page,
}) => {
  await login(page, "?sync=on");
  await openSettings(page);
  const syncSelect = settingsPage(page).getByLabel("Check for sync every");
  await expect(syncSelect).toBeEnabled();

  await selectValue(page, "Check for sync every", "15 minutes");
  await expect(syncSelect).toHaveText("15 minutes");

  await page.reload();
  await login(page, "?sync=on");
  await openSettings(page);
  await expect(
    settingsPage(page).getByLabel("Check for sync every"),
  ).toHaveText("15 minutes");
});

test("with the settings fault, a control reverts", async ({ page }) => {
  await login(page, "?slow=settings&fail=settings");
  await openSettings(page);

  const timeFormat = settingsPage(page).getByLabel("Time format");
  await expect(timeFormat).toHaveText("Automatic");

  await selectValue(page, "Time format", "24-hour");
  // Shown at once, optimistically...
  await expect(timeFormat).toHaveText("24-hour");
  // ...then reverted once the rejection's `reload()` reports the backend
  // never changed, with the alert `SettingsContext` doesn't show on its own.
  await expect(timeFormat).toHaveText("Automatic", { timeout: 5_000 });
  await expect(settingsPage(page).getByRole("alert")).toBeVisible();
});
