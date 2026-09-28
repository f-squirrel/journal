import { test, expect, type Page } from "./coverage";

// Encrypted images, entirely against the fallback (`src/api/fallback.ts`),
// which keeps media in a Map and mirrors the vault's rules. The fixture is a
// 1x1 PNG built from a constant, so no binary file is committed.

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

async function pickImage(page: Page, mimeType = "image/png", name = "a.png") {
  await page
    .getByTestId("image-input")
    .setInputFiles({ name, mimeType, buffer: PNG });
}

async function preview(page: Page) {
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Preview" }).click();
}

const LINK = /!\[\]\(media:[0-9a-f-]{36}\/[0-9a-f-]{36}\)/;

test("Insert image adds a media link to the text", async ({ page }) => {
  await login(page);
  await newEntry(page, "Pics", "hello");
  await pickImage(page);
  await expect(editor(page)).toContainText("](media:");
  await expect(editor(page)).toContainText(LINK);
});

test("pasting an image file adds a link", async ({ page }) => {
  await login(page);
  await newEntry(page, "Pasted");
  await editor(page).click();
  await editor(page).evaluate(
    (el, bytes) => {
      const data = new DataTransfer();
      data.items.add(
        new File([new Uint8Array(bytes)], "p.png", { type: "image/png" }),
      );
      el.dispatchEvent(
        new ClipboardEvent("paste", {
          clipboardData: data,
          bubbles: true,
          cancelable: true,
        }),
      );
    },
    [...PNG],
  );
  await expect(editor(page)).toContainText(LINK);
});

test("a failed save shows the error and inserts no link", async ({ page }) => {
  await login(page, "?fail=save-media");
  await newEntry(page, "Fails", "text");
  await pickImage(page);
  await expect(page.getByTestId("media-error")).toBeVisible();
  await expect(editor(page)).not.toContainText("media:");
});

test("a file that is not an image is refused", async ({ page }) => {
  await login(page);
  await newEntry(page, "Refused", "text");
  await pickImage(page, "text/plain", "a.txt");
  await expect(page.getByTestId("media-error")).toContainText("pictures");
  await expect(editor(page)).not.toContainText("media:");
});

test("text typed while an image saves stays, and the link lands where the paste was", async ({
  page,
}) => {
  await login(page, "?slow=save-media&slowMs=800");
  await newEntry(page, "Slow", "first");
  await pickImage(page);
  // The button inserts at the cursor as it is when the save finishes, so a
  // paste is what pins the position.
  await editor(page).press("Home");
  await editor(page).pressSequentially("ZZ");
  await expect(editor(page)).toContainText(LINK);
  // Typing kept its place, and the link went on a line of its own.
  await expect
    .poll(() => editor(page).innerText())
    .toMatch(/^ZZ\n!\[\]\(media:[0-9a-f-]{36}\/[0-9a-f-]{36}\)\nfirst$/);
});

test("switching entries while an image saves inserts nothing anywhere", async ({
  page,
}) => {
  await login(page, "?slow=save-media&slowMs=800");
  await newEntry(page, "Leaving", "stay");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  await pickImage(page);
  await page
    .getByTestId("entry-row")
    .filter({ hasText: "First day back" })
    .click();
  await page.waitForTimeout(1200);
  await expect(page.getByTestId("entry-body")).not.toContainText("media:");
  await page.getByTestId("entry-row").filter({ hasText: "Leaving" }).click();
  await expect(page.getByTestId("entry-body")).not.toContainText("media:");
});

async function entryWithImage(page: Page, title: string) {
  await newEntry(page, title, "look");
  await pickImage(page);
  await expect(editor(page)).toContainText(LINK);
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
}

test("the read view shows the image from a blob URL", async ({ page }) => {
  await login(page);
  await entryWithImage(page, "Shown");
  await preview(page);
  const image = page.getByTestId("media-image");
  await expect(image).toBeVisible();
  await expect(image).toHaveAttribute("src", /^blob:/);
});

test("an image stays the same DOM node across an unrelated re-render", async ({
  page,
}) => {
  await login(page);
  await entryWithImage(page, "Stable");
  await preview(page);
  const image = page.getByTestId("media-image");
  await expect(image).toBeVisible();
  await image.evaluate((el) => {
    (window as unknown as { keep: Element }).keep = el;
  });
  // Any re-render of the pane: opening and closing the actions menu.
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  const same = await image.evaluate(
    (el) => (window as unknown as { keep: Element }).keep === el,
  );
  expect(same).toBe(true);
});

test("an image that cannot be read shows the unavailable box", async ({
  page,
}) => {
  await login(page, "?fail=get-media");
  await entryWithImage(page, "Broken");
  await preview(page);
  await expect(page.getByTestId("media-unavailable")).toBeVisible();
});

test("a typed image path shows a hint and no image", async ({ page }) => {
  await login(page);
  await newEntry(page, "Path", "![x](/home/u/a.jpg)");
  await preview(page);
  await expect(page.getByTestId("image-hint")).toContainText(
    "Use Insert image to add pictures",
  );
  await expect(page.getByTestId("entry-body").locator("img")).toHaveCount(0);
});

test("a media link that is not an image renders as inert text", async ({
  page,
}) => {
  const warnings: string[] = [];
  page.on("console", (m) => warnings.push(m.text()));
  await login(page);
  const id = "11111111-1111-4111-8111-111111111111";
  await newEntry(page, "Link", `[x](media:${id}/${id})`);
  await preview(page);
  await expect(page.getByTestId("entry-body").locator("a")).not.toHaveAttribute(
    "href",
    /media:/,
  );
  expect(warnings.filter((w) => /validateDOMNesting/.test(w))).toEqual([]);
});

test("the history preview renders the image", async ({ page }) => {
  await login(page);
  await entryWithImage(page, "Hist");
  await editor(page).press("End");
  await editor(page).pressSequentially(" more");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Version history" }).click();
  const dialog = page.getByRole("dialog", { name: "Version history" });
  await dialog.getByText("Hist", { exact: true }).first().click();
  await expect(dialog.getByTestId("media-image")).toBeVisible();
});

test("an image whose entry was deleted is unavailable", async ({ page }) => {
  await login(page);
  await entryWithImage(page, "Owner");
  const link = (await editor(page).innerText()).match(LINK)![0];
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Delete" }).last().click();
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("Borrower");
  await editor(page).fill(link);
  await preview(page);
  await expect(page.getByTestId("media-unavailable")).toBeVisible();
});

test("a stale-save conflicted copy owns its own copy of the image", async ({
  page,
}) => {
  await page.clock.install();
  await login(page, "?sync=pull");
  await page
    .getByTestId("entry-row")
    .filter({ hasText: "First day back" })
    .click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await editor(page).fill("mine");
  await pickImage(page);
  await expect(editor(page)).toContainText(LINK);

  // A sync pass rewrites the entry under the open pane; the debounced save
  // then meets Stale and the draft becomes a copy.
  await page.getByTestId("sync-button").click();
  const cloud = page.getByRole("dialog", { name: "Cloud sync" });
  await cloud.getByRole("button", { name: "Sync now" }).click();
  await cloud.getByRole("button", { name: "Close" }).click();
  await page.clock.runFor(1000);

  const copy = page
    .getByTestId("entry-row")
    .filter({ hasText: "(conflicted copy)" });
  await expect(copy).toBeVisible();
  await copy.click();
  await expect(page.getByTestId("media-image")).toBeVisible();
  const href = page.url().match(/entries\/([0-9a-f-]{36})/)?.[1];
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  // The link now names the copy, not the entry it was made from.
  await expect(editor(page)).toContainText(`media:${href}/`);
});
