import { test, expect, type Page } from "./coverage";
import type { Settings, SettingsPatch } from "../src/types";

// Mirrors the ambient declaration in `src/api/fallback.ts`, which this file
// doesn't import (it only exists in the browser, reached through
// `page.evaluate`), so `tsc` here knows the shape of the debug hook.
declare global {
  interface Window {
    __settingsTestHooks?: {
      current(): Settings;
      calls: { patch: SettingsPatch; at: number }[];
      loads(): number;
    };
  }
}

// The plumbing: `SettingsContext` (one promise queue, `confirmed ⊕
// pending`, reload on rejection) sitting on top of `src/api/settings.ts` and
// the fallback's device/journal split. These tests drive the controls
// wired onto real settings directly — the rail's colour-mode toggle
// (`color_mode`) and the shortcuts dialog's rebind (`shortcuts`) — and the
// Settings page's own "Time format" control for the journal half.
// A debug hook the fallback exposes only for this file
// (`window.__settingsTestHooks`; see `src/api/fallback.ts`) fills the one gap
// no UI can: reading the backend's own settings while the vault is locked,
// since the Settings page sits behind unlock, and
// counting `getSettings` calls to prove a reload actually happened.

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

function sidebar(page: Page) {
  return page.getByTestId("sidebar");
}

/// "Light mode" means the theme is currently dark (the button offers the
/// opposite), and vice versa — see `Sidebar.tsx`. `includeHidden` reaches
/// through the `aria-hidden` MUI puts on the rest of the page while a modal
/// is open — used by the tests that need to check the colour without
/// closing the shortcuts dialog first, so a flick-back in between wouldn't
/// slip past unseen.
function modeToggle(page: Page, opts: { includeHidden?: boolean } = {}) {
  return sidebar(page).getByRole("button", {
    name: /^(Light mode|Dark mode)$/,
    includeHidden: opts.includeHidden,
  });
}

async function currentMode(
  page: Page,
  opts: { includeHidden?: boolean } = {},
): Promise<"light" | "dark"> {
  const label = await modeToggle(page, opts).getAttribute("aria-label");
  return label === "Light mode" ? "dark" : "light";
}

function shortcutsDialog(page: Page) {
  return page.getByRole("dialog", { name: "Keyboard shortcuts" });
}

async function openShortcuts(page: Page) {
  await sidebar(page)
    .getByRole("button", { name: "Keyboard shortcuts" })
    .click();
  await expect(shortcutsDialog(page)).toBeVisible();
}

async function recordChord(page: Page, id: string, chord: string) {
  await shortcutsDialog(page)
    .getByTestId(`shortcut-row-${id}`)
    .getByRole("button", { name: "Change" })
    .click();
  await expect(
    shortcutsDialog(page)
      .getByTestId(`shortcut-row-${id}`)
      .getByPlaceholder("Press keys…"),
  ).toBeFocused();
  await page.keyboard.press(chord);
}

/// The backend's own settings, bypassing `confirmed ⊕ pending` — see
/// `fallback.ts`.
function backendSettings(page: Page): Promise<Settings> {
  return page.evaluate(() => window.__settingsTestHooks!.current());
}

function backendCalls(
  page: Page,
): Promise<{ patch: SettingsPatch; at: number }[]> {
  return page.evaluate(() => window.__settingsTestHooks!.calls);
}

test("device settings survive a reload", async ({ page }) => {
  await login(page);
  const before = await currentMode(page);

  await modeToggle(page).click();
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Dark mode" : "Light mode",
  );

  await page.reload();
  await login(page);
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Dark mode" : "Light mode",
  );
});

test("colour mode system follows the OS setting", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "journal.fallback.device",
      JSON.stringify({ color_mode: "system" }),
    );
  });
  await page.emulateMedia({ colorScheme: "dark" });
  await login(page);

  await expect(modeToggle(page)).toHaveAccessibleName("Light mode");

  await page.emulateMedia({ colorScheme: "light" });
  await expect(modeToggle(page)).toHaveAccessibleName("Dark mode");
});

test("a rejected update shows the backend's value again", async ({ page }) => {
  // `?slow=` holds the rejection open long enough for the optimistic flip to
  // actually paint before it reverts — with no delay, a same-tick rejection
  // could revert before a frame is ever drawn, and the flip would go
  // unchecked.
  await login(page, "?slow=settings&fail=settings");
  const before = await currentMode(page);

  await modeToggle(page).click();

  // Shown at once, optimistically...
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Dark mode" : "Light mode",
  );
  // ...then reverted once the rejection's `reload()` reports the backend
  // never changed.
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Light mode" : "Dark mode",
    { timeout: 5_000 },
  );
  expect(await currentMode(page)).toBe(before);
});

test("a colour change and, at once, a shortcut change: both show at once, and the second request waits for the first", async ({
  page,
}) => {
  await login(page, "?slow=settings-first&slowMs=8000");
  const before = await currentMode(page);

  await modeToggle(page).click();
  // Shown at once, well before the slow call could have settled (`SLOW_MS`
  // in `fallback.ts` is 1200 ms). Checked before the dialog opens: MUI marks
  // the rest of the page `aria-hidden` while a modal is open, so the rail
  // isn't queryable by role once `openShortcuts` runs.
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Dark mode" : "Light mode",
  );

  await openShortcuts(page);
  await recordChord(page, "entry.new", "ControlOrMeta+Shift+n");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    shortcutsDialog(page)
      .getByTestId("shortcut-row-entry.new")
      .getByTestId("shortcut-keys"),
  ).toContainText(/Shift|⇧/);

  // The shortcut request is not sent until the colour one has settled: right
  // after both clicks, only one call has reached the fallback.
  expect((await backendCalls(page)).length).toBe(1);

  await expect
    .poll(async () => (await backendCalls(page)).length, { timeout: 15_000 })
    .toBe(2);
  const calls = await backendCalls(page);
  expect(calls[0].patch).toHaveProperty("color_mode");
  expect(calls[1].patch).toHaveProperty("shortcuts");
  // The second call started only once the first had settled — `?slow=
  // settings-first` only delays that first call, so this gap is the queue,
  // not another delay stacked on top.
  expect(calls[1].at - calls[0].at).toBeGreaterThanOrEqual(1000);

  // Right at that moment — dialog still open, second request just sent —
  // both values are still as set. The rebind is inside the dialog, so it's
  // checked normally; the colour toggle is a background sibling MUI marks
  // `aria-hidden` while the modal is open, so it's checked with
  // `includeHidden` instead of closing the dialog first, which would let a
  // flick-back between the two calls slip past unseen.
  await expect(
    shortcutsDialog(page)
      .getByTestId("shortcut-row-entry.new")
      .getByTestId("shortcut-keys"),
  ).toContainText(/Shift|⇧/);
  expect(await currentMode(page, { includeHidden: true })).not.toBe(before);

  // Neither value flicks back once the slow response lands, and both are
  // still there after closing the dialog and reloading.
  await shortcutsDialog(page).getByRole("button", { name: "Close" }).click();
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Dark mode" : "Light mode",
  );

  await page.reload();
  await login(page, "?slow=settings-first");
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Dark mode" : "Light mode",
  );
  await openShortcuts(page);
  await expect(
    shortcutsDialog(page)
      .getByTestId("shortcut-row-entry.new")
      .getByTestId("shortcut-keys"),
  ).toContainText(/Shift|⇧/);
});

test("two overlapping updates that both fail both revert, and the newer rejection doesn't restore the older one's optimistic value", async ({
  page,
}) => {
  await login(page, "?slow=settings&fail=settings");
  const before = await currentMode(page);

  await modeToggle(page).click();
  // Shown at once, checked before the dialog opens — see the previous test
  // for why the rail isn't queryable by role once it does.
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Dark mode" : "Light mode",
  );

  await openShortcuts(page);
  await recordChord(page, "entry.new", "ControlOrMeta+Shift+n");
  await expect(
    shortcutsDialog(page)
      .getByTestId("shortcut-row-entry.new")
      .getByTestId("shortcut-keys"),
  ).toContainText(/Shift|⇧/);

  // Both requests fail (`?fail=settings`) and each rejection queues its own
  // `reload()`; give both slow calls (`?slow=settings` delays every one) and
  // both reloads time to settle.
  await expect(
    shortcutsDialog(page)
      .getByTestId("shortcut-row-entry.new")
      .getByTestId("shortcut-keys"),
  ).not.toContainText(/Shift|⇧/, { timeout: 8_000 });

  await shortcutsDialog(page).getByRole("button", { name: "Close" }).click();
  await expect(modeToggle(page)).toHaveAccessibleName(
    before === "dark" ? "Light mode" : "Dark mode",
  );
  expect(await currentMode(page)).toBe(before);
});

function backendLoads(page: Page): Promise<number> {
  return page.evaluate(() => window.__settingsTestHooks!.loads());
}

test("journal settings read as defaults after lock and come back after unlock", async ({
  page,
}) => {
  await login(page);
  const initial = await backendSettings(page);

  // Written through the real control now that the Settings page has one,
  // rather than the debug hook `settings-store.spec.ts` needed before it
  // existed. The hook stays for what still has no UI: reading the backend's
  // own value while locked (the Settings page sits behind unlock — see the
  // plan, Non-goals) and counting `getSettings` calls, below.
  await page.getByRole("button", { name: "Settings" }).click();
  const timeFormatField = page
    .getByTestId("settings-page")
    .getByLabel("Time format");
  const nextLabel = initial.time_format === "h24" ? "12-hour" : "24-hour";
  await timeFormatField.click();
  await page.getByRole("option", { name: nextLabel }).click();
  await expect(timeFormatField).toHaveText(nextLabel);

  const changed = await backendSettings(page);
  expect(changed.time_format).not.toBe(initial.time_format);

  // The state check alone would pass even if `AppStateContext` never called
  // `useSettings().reload()` on lock and unlock: the fallback's journal half
  // never leaves memory, and `currentSettings()` masks it to defaults purely
  // from `locked`, with or without a fresh `getSettings` call. Counting
  // loads is what actually proves the context re-fetched.
  const loadsBeforeLock = await backendLoads(page);
  await page.getByRole("button", { name: "Log out" }).click();
  await expect(page.getByLabel("Passphrase")).toBeVisible();
  await expect.poll(() => backendLoads(page)).toBeGreaterThan(loadsBeforeLock);
  expect((await backendSettings(page)).time_format).toBe("locale");

  const loadsBeforeUnlock = await backendLoads(page);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
  await expect
    .poll(() => backendLoads(page))
    .toBeGreaterThan(loadsBeforeUnlock);
  expect((await backendSettings(page)).time_format).toBe(changed.time_format);

  // And the Settings page itself shows the value that survived the round
  // trip, not just the backend the hook reads directly.
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(
    page.getByTestId("settings-page").getByLabel("Time format"),
  ).toHaveText(nextLabel);
});
