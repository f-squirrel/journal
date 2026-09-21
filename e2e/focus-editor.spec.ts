import { test, expect, type Page } from "./coverage";

// Pins where the cursor is whenever an entry is in edit mode: in the markdown
// editor, so someone can type without clicking. It passes on the code as it
// stands (`EntryPane` hands `MarkdownEditor` `autoFocus`); it is here so a
// change to the menu, the mode switch or the editor's mount cannot quietly
// take the cursor away.
//
// Every case types with `page.keyboard` and never clicks the editor first,
// because a click would focus it whatever the app did.

// Fills the field explicitly rather than relying on it being focused already:
// this file is about the editor, not the login screen.
async function login(page: Page) {
  await page.goto("/");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

/// The entry body, rendered or in the editor — same test id either way.
function body(page: Page) {
  return page.getByTestId("entry-body");
}

/// The editor itself: CodeMirror is a contenteditable, so its value is its
/// text and it is the element that holds focus.
function editor(page: Page) {
  return body(page).locator(".cm-content");
}

async function openEditor(page: Page) {
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
}

async function openPreview(page: Page) {
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Preview" }).click();
}

test("a new entry has the cursor in the editor", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();

  await expect(editor(page)).toBeFocused();
  await page.keyboard.type("hello");
  await expect(editor(page)).toContainText("hello");
});

test("an entry opens in read mode with no editor to focus", async ({
  page,
}) => {
  await login(page);
  await page.getByText("First day back").click();

  await expect(body(page)).toContainText("Started journaling again");
  await expect(editor(page)).toHaveCount(0);
});

test("switching an entry to edit mode puts the cursor at the end of it", async ({
  page,
}) => {
  await login(page);
  await page.getByText("First day back").click();
  await openEditor(page);

  await expect(editor(page)).toBeFocused();
  // No newline: Enter at the end of a list item would continue the list.
  await page.keyboard.type(" ZZEND");
  await expect(editor(page)).toHaveText(/ZZEND$/);
});

test("the cursor returns to the editor each time it is switched back to", async ({
  page,
}) => {
  await login(page);
  await page.getByText("First day back").click();
  await openEditor(page);
  await expect(editor(page)).toBeFocused();

  await openPreview(page);
  await expect(editor(page)).toHaveCount(0);

  // The editor is a new instance the second time round.
  await openEditor(page);
  await expect(editor(page)).toBeFocused();
  await page.keyboard.type(" AGAIN");
  await expect(editor(page)).toHaveText(/AGAIN$/);
});

test("the title can still be clicked into and typed in", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "New entry" }).first().click();
  await expect(editor(page)).toBeFocused();

  const title = page.getByPlaceholder("Title");
  await title.click();
  await expect(title).toBeFocused();
  await page.keyboard.type("A title");

  await expect(title).toHaveValue("A title");
  await expect(editor(page)).not.toBeFocused();
});
