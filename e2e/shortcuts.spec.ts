import { test, expect, type Page } from "./coverage";

// The shortcuts registry (`src/shortcuts/`): `search.focus`, then `entry.new`,
// `entry.toggleMode`, `vault.lock` and `view.calendar` with their chords on
// the rail and the `⋯` menu, then the read-only dialog and rebinding.
//
// `page.keyboard.press("ControlOrMeta+n")` and `"...+l"` were checked against
// Playwright's Chromium (headless, the default here): unlike a real desktop
// Chrome window, it has no browser chrome to intercept them for "new window"
// or "focus the address bar" — the same reason the pre-existing Ctrl-F test
// above already worked before this plan. Both reach the page as ordinary
// keydown events, so no synthetic dispatch is needed for them.

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

/// The editor itself: CodeMirror is a contenteditable, so its value is its
/// text and it is the element that holds focus.
function editor(page: Page) {
  return page.getByTestId("entry-body").locator(".cm-content");
}

/// `$mod+F`, but dispatched as a Russian keyboard layout would report it: the
/// physical key is still `KeyF` (`event.code`, which doesn't change with
/// layout), but `event.key` is the Cyrillic letter under that key, "а", not
/// "f". `page.keyboard.press` can't produce this — it types characters, not
/// physical keys — so the event is dispatched by hand, the same way a real
/// non-Latin keyboard would deliver it to the page.
async function pressNonLatinModF(page: Page) {
  await page.evaluate(() => {
    // Mirrors `isApple()`/tinykeys' own platform test, so this fires the
    // same modifier a real key press would on whatever platform the test
    // runs on.
    const apple = /Mac|iPod|iPhone|iPad/.test(navigator.platform);
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "а",
        code: "KeyF",
        ctrlKey: !apple,
        metaKey: apple,
        bubbles: true,
      }),
    );
  });
}

/// The bug this migration fixes: the old listener checked `event.key ===
/// "f"`, which is never true under a Russian layout. `search.focus` matches
/// on `event.code` instead, so it has to work regardless of what `event.key`
/// says.
test("search.focus reaches the field from a non-Latin keyboard layout", async ({
  page,
}) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await expect(editor(page)).toBeFocused();

  await pressNonLatinModF(page);

  await expect(page.getByLabel("Search entries")).toBeFocused();
});

// entry.new, entry.toggleMode, vault.lock, view.calendar.

test("ControlOrMeta+n opens a new draft in edit mode", async ({ page }) => {
  await login(page);

  await page.keyboard.press("ControlOrMeta+n");

  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);
  await expect(editor(page)).toBeVisible();
  await expect(editor(page)).toBeFocused();
});

test("ControlOrMeta+e toggles edit and read, including from inside the editor, and saves what was typed", async ({
  page,
}) => {
  await login(page);
  await page.getByText("First day back").click();
  await expect(page.getByTestId("entry-body")).toBeVisible();

  await page.keyboard.press("ControlOrMeta+e");
  await expect(editor(page)).toBeVisible();

  await editor(page).fill("Edited through the shortcut.");
  // The editor's own contenteditable still holds focus here: this is the
  // capture listener beating CodeMirror's keymap, not just the chord working
  // from elsewhere on the page.
  await expect(editor(page)).toBeFocused();
  await page.keyboard.press("ControlOrMeta+e");

  // Back in read mode: the editor is gone and `stopEditing`'s `saveNow` ran
  // before the switch, so the typed text is what's rendered.
  await expect(
    page.getByTestId("entry-body").locator(".cm-content"),
  ).toHaveCount(0);
  await expect(page.getByTestId("entry-body")).toContainText(
    "Edited through the shortcut.",
  );

  await page.keyboard.press("ControlOrMeta+e");
  await expect(editor(page)).toBeVisible();
  await expect(editor(page)).toContainText("Edited through the shortcut.");
});

test("ControlOrMeta+l locks the vault and returns to the login screen", async ({
  page,
}) => {
  await login(page);

  await page.keyboard.press("ControlOrMeta+l");

  await expect(page).toHaveURL(/#\/login$/);
  await expect(page.getByLabel("Passphrase")).toBeVisible();
});

test("ControlOrMeta+Shift+c opens the calendar", async ({ page }) => {
  await login(page);

  await page.keyboard.press("ControlOrMeta+Shift+c");

  await expect(page.getByRole("heading", { name: "Calendar" })).toBeVisible();
});

test("shortcuts do nothing on the login screen", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByLabel("Passphrase")).toBeVisible();

  await page.keyboard.press("ControlOrMeta+n");

  await expect(page).toHaveURL(/#\/login$/);
  await expect(page.getByLabel("Passphrase")).toBeVisible();
});

test("the rail buttons for entry.new, view.calendar and vault.lock expose their shortcut", async ({
  page,
}) => {
  await login(page);
  // Scoped to the rail: the pane also has its own "New entry" text button
  // (`NoEntrySelected`), which is not a `RailButton` and carries no shortcut.
  const sidebar = page.getByTestId("sidebar");

  await expect(
    sidebar.getByRole("button", { name: "New entry", exact: true }),
  ).toHaveAttribute("aria-keyshortcuts", /^(Control|Meta)\+N$/);
  // Loose match: its label also carries the "on this day" count, when there
  // is one (see e2e/calendar.spec.ts).
  await expect(
    sidebar.getByRole("button", { name: /^Calendar/ }),
  ).toHaveAttribute("aria-keyshortcuts", /^(Control|Meta)\+Shift\+C$/);
  await expect(
    sidebar.getByRole("button", { name: "Log out", exact: true }),
  ).toHaveAttribute("aria-keyshortcuts", /^(Control|Meta)\+L$/);
});

test("a shortcut closes the drawer if it's open, on a phone", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page);
  await page.getByRole("button", { name: "Open menu" }).click();
  await expect(page.getByTestId("sidebar")).toBeVisible();

  await page.keyboard.press("ControlOrMeta+n");

  // The drawer unmounts its content when closed.
  await expect(page.getByTestId("sidebar")).toHaveCount(0);
  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);
});

test("shortcuts still fire on a phone with the list hidden behind an open pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page);
  await page.getByText("First day back").click();
  await expect(page.getByTestId("entry-list")).toHaveCount(0);

  await page.keyboard.press("ControlOrMeta+l");

  await expect(page).toHaveURL(/#\/login$/);
});

// The read-only shortcuts dialog.

const ACTION_IDS = [
  "entry.new",
  "search.focus",
  "entry.toggleMode",
  "vault.lock",
  "view.calendar",
  "view.settings",
  "help.shortcuts",
];

function shortcutsDialog(page: Page) {
  return page.getByRole("dialog", { name: "Keyboard shortcuts" });
}

test("ControlOrMeta+/ opens the shortcuts dialog from the entry list", async ({
  page,
}) => {
  await login(page);

  await page.keyboard.press("ControlOrMeta+/");

  await expect(shortcutsDialog(page)).toBeVisible();
});

test("ControlOrMeta+/ opens the shortcuts dialog from inside the editor, beating CodeMirror's own binding, and leaves the entry's text unchanged", async ({
  page,
}) => {
  await login(page);
  await page.getByText("First day back").click();
  await expect(page.getByTestId("entry-body")).toBeVisible();
  await page.keyboard.press("ControlOrMeta+e");
  await expect(editor(page)).toBeFocused();
  const before = await editor(page).textContent();

  await page.keyboard.press("ControlOrMeta+/");

  // If the dialog didn't beat CodeMirror's `Mod-/` (`toggleComment`), the
  // editor would still have focus and its text would have a comment marker
  // inserted instead.
  await expect(shortcutsDialog(page)).toBeVisible();
  await expect(editor(page)).toContainText(before ?? "");
});

test("the shortcuts dialog lists all seven actions with their default chords", async ({
  page,
}) => {
  await login(page);

  await page.keyboard.press("ControlOrMeta+/");
  const dialog = shortcutsDialog(page);
  await expect(dialog).toBeVisible();

  for (const id of ACTION_IDS) {
    await expect(dialog.getByTestId(`shortcut-row-${id}`)).toBeVisible();
  }
  const keys = dialog.getByTestId("shortcut-keys");
  await expect(keys).toHaveCount(ACTION_IDS.length);
  // None of the seven defaults is unbound, so no row should show the
  // placeholder used for a `null` binding.
  for (const text of await keys.allTextContents()) {
    expect(text).not.toBe("—");
  }
});

test("shortcuts other than help.shortcuts do nothing while the dialog is open", async ({
  page,
}) => {
  await login(page);

  await page.keyboard.press("ControlOrMeta+/");
  await expect(shortcutsDialog(page)).toBeVisible();

  await page.keyboard.press("ControlOrMeta+n");

  // Still on the entry list, dialog still open: `entry.new`'s handler never
  // ran because a dialog was open when the keydown fired. (The check is
  // "is any dialog open", not "is the keydown's target inside one" — see
  // `ShortcutsContext.tsx` for why: MUI's `FocusTrap` autofocuses its own
  // wrapping container the instant a dialog opens, and that container is
  // the *parent* of the `role="dialog"` element, not a descendant of it.)
  await expect(shortcutsDialog(page)).toBeVisible();
  await expect(page).toHaveURL(/#\/entries$/);
});

test("the rail button opens the shortcuts dialog", async ({ page }) => {
  await login(page);
  const sidebar = page.getByTestId("sidebar");

  await sidebar.getByRole("button", { name: "Keyboard shortcuts" }).click();

  await expect(shortcutsDialog(page)).toBeVisible();
});

// Overrides are the `shortcuts` field of the device settings, in the
// fallback's `localStorage["journal.fallback.device"]`
// (`DeviceSettings.shortcuts`), read through `useSettings()` rather than a
// dedicated storage key. Each override is seeded with `page.addInitScript`,
// which runs before any of the app's own scripts, so the first `getSettings`
// call sees it.

async function seedDeviceSettings(page: Page, device: Record<string, unknown>) {
  await page.addInitScript((json) => {
    window.localStorage.setItem("journal.fallback.device", json);
  }, JSON.stringify(device));
}

test("an override rebinds the action, and the default chord it replaced stops working", async ({
  page,
}) => {
  await seedDeviceSettings(page, {
    shortcuts: { "entry.new": "$mod+Shift+KeyN" },
  });
  await login(page);

  await page.keyboard.press("ControlOrMeta+n");
  await expect(page).toHaveURL(/#\/entries$/);

  await page.keyboard.press("ControlOrMeta+Shift+n");
  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);
});

test("a null override unbinds the action entirely", async ({ page }) => {
  await seedDeviceSettings(page, { shortcuts: { "entry.new": null } });
  await login(page);

  await page.keyboard.press("ControlOrMeta+n");

  await expect(page).toHaveURL(/#\/entries$/);
});

test("garbage JSON in storage is ignored and every default still works", async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.localStorage.setItem("journal.fallback.device", "not json");
  });
  await login(page);

  await page.keyboard.press("ControlOrMeta+n");

  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);
});

test("an unknown action id and a non-string binding are dropped, and the known default still works", async ({
  page,
}) => {
  await seedDeviceSettings(page, {
    shortcuts: { nope: "x", "entry.new": 5 },
  });
  await login(page);

  await page.keyboard.press("ControlOrMeta+n");

  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);
});

test("a stored override on an editor-owned chord is dropped", async ({
  page,
}) => {
  await seedDeviceSettings(page, {
    shortcuts: { "entry.new": "Alt+ArrowUp" },
  });
  await login(page);

  await page.keyboard.press("ControlOrMeta+n");

  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);
});

test("a stored override that clashes with another action's chord is unbound, leaving that action on it", async ({
  page,
}) => {
  await seedDeviceSettings(page, {
    shortcuts: { "vault.lock": "$mod+KeyN" },
  });
  await login(page);

  await page.keyboard.press("ControlOrMeta+n");
  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);

  await page.keyboard.press("ControlOrMeta+/");
  await expect(
    shortcutRow(page, "vault.lock").getByTestId("shortcut-keys"),
  ).toHaveText("—");
});

// The old test drove a `localStorage.setItem` that threw (a full quota, a
// private-mode browser); overrides now live behind `getSettings`, whose
// backend equivalent is a settings load, so the same "the app still works
// on broken storage" guarantee is exercised through `?fail=settings-load`
// instead: the app renders with
// `LOAD_FAILED_SETTINGS` — default bindings, dark mode, and the idle lock
// still active.
test("a failed settings load doesn't take the app down: default bindings, dark mode, idle lock active", async ({
  page,
}) => {
  // `LOAD_FAILED_SETTINGS` (`SettingsContext.tsx`): dark mode, default
  // bindings, and `idle_lock_minutes: 5` — never `null`, so the lock stays
  // on even though settings could not be read.
  await page.clock.install();
  await login(page, "?fail=settings-load");

  await page.keyboard.press("ControlOrMeta+n");
  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);

  const sidebar = page.getByTestId("sidebar");
  await expect(
    sidebar.getByRole("button", { name: "Light mode" }),
  ).toBeVisible();

  await page.clock.runFor(5 * 60_000);
  await expect(page.getByLabel("Passphrase")).toBeVisible();
});

test("the shortcuts dialog works at phone width, opened from the drawer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page);
  await page.getByRole("button", { name: "Open menu" }).click();
  const sidebar = page.getByTestId("sidebar");
  await expect(sidebar).toBeVisible();

  await sidebar.getByRole("button", { name: "Keyboard shortcuts" }).click();

  // The drawer closes and the dialog, owned by JournalLayout, survives the
  // sidebar unmounting.
  await expect(page.getByTestId("sidebar")).toHaveCount(0);
  await expect(shortcutsDialog(page)).toBeVisible();
  for (const id of ACTION_IDS) {
    await expect(
      shortcutsDialog(page).getByTestId(`shortcut-row-${id}`),
    ).toBeVisible();
  }
});

// Rebinding through the dialog. Each row gets Change, Clear and a
// conditional Reset; the footer gets Reset all.

function shortcutRow(page: Page, id: string) {
  return shortcutsDialog(page).getByTestId(`shortcut-row-${id}`);
}

function noOverflow(page: Page) {
  return page.evaluate(
    () =>
      document.documentElement.scrollWidth <=
      document.documentElement.clientWidth,
  );
}

/// Clicks a row's Change button, waits for the recorder to take focus, then
/// presses `chord`. Doesn't assert the outcome — refused chords need to
/// inspect the Alert first.
async function recordChord(page: Page, id: string, chord: string) {
  await shortcutRow(page, id).getByRole("button", { name: "Change" }).click();
  await expect(
    shortcutRow(page, id).getByPlaceholder("Press keys…"),
  ).toBeFocused();
  await page.keyboard.press(chord);
}

test("rebinding New entry through the dialog replaces the default, and the change survives a reload", async ({
  page,
}) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");
  await expect(shortcutsDialog(page)).toBeVisible();

  await recordChord(page, "entry.new", "ControlOrMeta+Shift+n");

  // No refusal, and the recorder is gone, replaced by the new chord.
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    shortcutRow(page, "entry.new").getByPlaceholder("Press keys…"),
  ).toHaveCount(0);

  await shortcutsDialog(page).getByRole("button", { name: "Close" }).click();
  await expect(shortcutsDialog(page)).toHaveCount(0);

  // The old chord is dead; the new one works.
  await page.keyboard.press("ControlOrMeta+n");
  await expect(page).toHaveURL(/#\/entries$/);
  await page.keyboard.press("ControlOrMeta+Shift+n");
  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);

  // The override lives in localStorage, not component state, so it survives
  // a reload and logging back in.
  await page.reload();
  await login(page);
  await page.keyboard.press("ControlOrMeta+Shift+n");
  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);
});

test("rebinding to a chord another action already uses is refused, naming that action, and the recorder stays open", async ({
  page,
}) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");

  await recordChord(page, "entry.new", "ControlOrMeta+e");

  await expect(page.getByRole("alert")).toContainText("Switch edit / read");
  await expect(
    shortcutRow(page, "entry.new").getByPlaceholder("Press keys…"),
  ).toBeFocused();
});

test("rebinding to a bare letter is refused for carrying no modifier", async ({
  page,
}) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");

  await recordChord(page, "entry.new", "n");

  await expect(page.getByRole("alert")).toContainText(
    "a single key would fire while you type",
  );
  await expect(
    shortcutRow(page, "entry.new").getByPlaceholder("Press keys…"),
  ).toBeFocused();
});

test("rebinding to a chord the editor owns is refused", async ({ page }) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");

  await recordChord(page, "entry.new", "ControlOrMeta+z");

  await expect(page.getByRole("alert")).toContainText("used by the editor");
  await expect(
    shortcutRow(page, "entry.new").getByPlaceholder("Press keys…"),
  ).toBeFocused();
});

test("rebinding to an Alt chord the editor owns is refused", async ({
  page,
}) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");

  await recordChord(page, "entry.new", "Alt+ArrowUp");

  await expect(page.getByRole("alert")).toContainText("used by the editor");
});

test("a bare Escape cancels the recorder without closing the dialog", async ({
  page,
}) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");
  await shortcutRow(page, "entry.new")
    .getByRole("button", { name: "Change" })
    .click();
  await expect(
    shortcutRow(page, "entry.new").getByPlaceholder("Press keys…"),
  ).toBeFocused();

  await page.keyboard.press("Escape");

  await expect(shortcutsDialog(page)).toBeVisible();
  await expect(
    shortcutRow(page, "entry.new").getByPlaceholder("Press keys…"),
  ).toHaveCount(0);
  await expect(
    shortcutRow(page, "entry.new").getByRole("button", { name: "Change" }),
  ).toBeVisible();
});

test("losing focus cancels the recorder", async ({ page }) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");
  await shortcutRow(page, "entry.new")
    .getByRole("button", { name: "Change" })
    .click();
  await expect(
    shortcutRow(page, "entry.new").getByPlaceholder("Press keys…"),
  ).toBeFocused();

  // A click on the title bar isn't on anything focusable, but it still moves
  // focus off the recorder — the same as a click anywhere else in the app.
  await shortcutsDialog(page)
    .getByRole("heading", { name: "Keyboard shortcuts" })
    .click();

  await expect(
    shortcutRow(page, "entry.new").getByPlaceholder("Press keys…"),
  ).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("Clear, Reset and Reset all restore what they should", async ({
  page,
}) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");
  const row = shortcutRow(page, "entry.new");

  // Not overridden yet: no Reset button.
  await expect(row.getByRole("button", { name: "Reset" })).toHaveCount(0);

  await recordChord(page, "entry.new", "ControlOrMeta+Shift+n");
  await expect(row.getByTestId("shortcut-keys")).toContainText(/Shift|⇧/);
  await expect(row.getByRole("button", { name: "Reset" })).toBeVisible();

  await row.getByRole("button", { name: "Clear" }).click();
  await expect(row.getByTestId("shortcut-keys")).toHaveText("—");
  // Explicitly unbound is still a departure from the default, so Reset stays.
  await expect(row.getByRole("button", { name: "Reset" })).toBeVisible();

  await row.getByRole("button", { name: "Reset" }).click();
  await expect(row.getByTestId("shortcut-keys")).toHaveText(/^(Ctrl\+N|⌘N)$/);
  await expect(row.getByRole("button", { name: "Reset" })).toHaveCount(0);

  // Reset all, after overriding two different actions.
  await recordChord(page, "entry.new", "ControlOrMeta+Shift+n");
  const lockRow = shortcutRow(page, "vault.lock");
  await lockRow.getByRole("button", { name: "Clear" }).click();
  await expect(lockRow.getByTestId("shortcut-keys")).toHaveText("—");

  await shortcutsDialog(page)
    .getByRole("button", { name: "Reset all" })
    .click();

  await expect(row.getByTestId("shortcut-keys")).toHaveText(/^(Ctrl\+N|⌘N)$/);
  await expect(row.getByRole("button", { name: "Reset" })).toHaveCount(0);
  await expect(lockRow.getByTestId("shortcut-keys")).not.toHaveText("—");
  await expect(lockRow.getByRole("button", { name: "Reset" })).toHaveCount(0);

  // Reset all actually took effect on the live bindings, not just the rows —
  // close the dialog first, since shortcuts are suppressed while one is open.
  await shortcutsDialog(page).getByRole("button", { name: "Close" }).click();
  await expect(shortcutsDialog(page)).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+n");
  await expect(page).toHaveURL(/#\/entries\/[0-9a-f-]{36}$/);
});

test("Reset is refused while another action holds the default chord", async ({
  page,
}) => {
  await login(page);
  await page.keyboard.press("ControlOrMeta+/");
  const row = shortcutRow(page, "entry.new");
  await row.getByRole("button", { name: "Clear" }).click();
  await recordChord(page, "vault.lock", "ControlOrMeta+n");
  await expect(
    shortcutRow(page, "vault.lock").getByPlaceholder("Press keys…"),
  ).toHaveCount(0);

  await row.getByRole("button", { name: "Reset" }).click();

  await expect(page.getByRole("alert")).toContainText("Lock journal");
  await expect(row.getByTestId("shortcut-keys")).toHaveText("—");
});

test("Change, Clear and Reset stay usable at phone width", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page);
  await page.getByRole("button", { name: "Open menu" }).click();
  await page
    .getByTestId("sidebar")
    .getByRole("button", { name: "Keyboard shortcuts" })
    .click();
  await expect(shortcutsDialog(page)).toBeVisible();

  await recordChord(page, "entry.new", "ControlOrMeta+Shift+n");

  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    shortcutRow(page, "entry.new").getByRole("button", { name: "Reset" }),
  ).toBeVisible();
  expect(await noOverflow(page)).toBe(true);
});

test("the rebind controls stay usable at a 24px root font", async ({
  page,
}) => {
  await login(page);
  await page.addStyleTag({ content: "html { font-size: 24px }" });

  await page.keyboard.press("ControlOrMeta+/");
  const row = shortcutRow(page, "entry.new");
  await expect(row.getByRole("button", { name: "Change" })).toBeVisible();
  await expect(row.getByRole("button", { name: "Clear" })).toBeVisible();

  await recordChord(page, "entry.new", "ControlOrMeta+Shift+n");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(row.getByRole("button", { name: "Reset" })).toBeVisible();
  expect(await noOverflow(page)).toBe(true);
});
