import { test, expect, type Page } from "@playwright/test";

// These tests run against the Vite dev server without Tauri's `invoke` bridge,
// so the app falls back to its seeded in-memory entries (see
// context/AppStateContext.tsx). They exercise frontend logic and navigation,
// not the Rust storage backend.
//
// The UI has no Save button: the entry pane autosaves, so the assertions wait
// on its save status rather than clicking anything.

async function login(page: Page) {
  await page.goto("/");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "All entries" })).toBeVisible();
}

/// The entry body, rendered or in the editor — same test id either way.
function body(page: Page) {
  return page.getByTestId("entry-body");
}

/// The editor itself: CodeMirror is a contenteditable rather than a textarea,
/// so there is no placeholder to find it by and its value is its text.
function editor(page: Page) {
  return body(page).locator(".cm-content");
}

/// Entries open rendered; editing is a deliberate switch from the menu.
async function openEditor(page: Page) {
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
}

async function openPreview(page: Page) {
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Preview" }).click();
}

function row(page: Page, title: string) {
  return page.getByTestId("entry-row").filter({ hasText: title });
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
  // The pane's own title, not the `# First day back` the entry's markdown
  // renders as an h1 below it.
  await expect(page.getByRole("heading", { name: "First day back", level: 2 })).toBeVisible();
  await expect(page.getByText("Started journaling again.")).toBeVisible();
});

test("an entry opens rendered, not as markdown", async ({ page }) => {
  await login(page);
  await page.getByText("First day back").click();
  await expect(body(page)).toContainText("Started journaling again");
  await expect(body(page)).not.toContainText("# First day back");
  await expect(body(page)).not.toContainText("- Coffee");
});

test("the menu switches between editing and preview", async ({ page }) => {
  await login(page);
  await page.getByText("First day back").click();

  await openEditor(page);
  // The editor is the markdown itself, syntax and all.
  await expect(editor(page)).toContainText("# First day back");
  await expect(editor(page)).toContainText("- Coffee");

  await openPreview(page);
  await expect(body(page)).not.toContainText("# First day back");
  await expect(editor(page)).toHaveCount(0);

  // And back: the menu offers Edit again, not a second Preview.
  await page.getByRole("button", { name: "Entry actions" }).click();
  await expect(page.getByRole("menuitem", { name: "Edit" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Preview" })).toHaveCount(0);
});

test("creating a new entry adds it to the list", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("E2E test entry");
  await editor(page).fill("# Hello from Playwright");

  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  await expect(row(page, "E2E test entry")).toBeVisible();
});

test("an untouched draft is never written", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  // Straight back out without typing: the journal must not grow a blank row.
  await page.getByText("Project ideas").click();
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
});

test("deleting an entry removes it from the list", async ({ page }) => {
  await login(page);
  await page.getByText("Project ideas").click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Delete entry" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
  await expect(page.getByText("Project ideas")).toHaveCount(0);
});

test("a new entry saves where crypto.randomUUID is unavailable", async ({ page }) => {
  // The iOS/macOS Tauri origin (`tauri://localhost`) is not a secure context,
  // so this secure-context-only API is missing there.
  await page.addInitScript(() => {
    // @ts-expect-error deleting a DOM API on purpose
    delete Object.getPrototypeOf(window.crypto).randomUUID;
  });
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("No randomUUID here");
  await editor(page).fill("# Still saves");

  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  await expect(row(page, "No randomUUID here")).toBeVisible();
});

test("switching entries flushes what was typed", async ({ page }) => {
  await login(page);
  await page.getByText("First day back").click();
  await openEditor(page);
  await editor(page).fill("# Flushed on switch");

  // Straight to another entry, without pausing for the debounce and without
  // switching back to preview: the pane unmounts, and its cleanup is what has
  // to write the draft.
  await page.getByText("Project ideas").click();
  await expect(body(page)).toContainText("Some things I want to build");

  await page.getByText("First day back").click();
  await expect(body(page)).toContainText("Flushed on switch");
});

test("an edited entry moves to the top of the list", async ({ page }) => {
  await login(page);
  // The older of the two seeded entries, so it starts second in the list the
  // backend's order (most-recently-updated first) puts them in.
  await page.getByText("First day back").click();
  await openEditor(page);
  await editor(page).fill("# Edited just now");

  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  await expect(page.getByTestId("entry-row").first()).toContainText("First day back");
});
