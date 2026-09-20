import { test, expect, type Page } from "./coverage";

// The export flow, over the fallback (`src/api/fallback.ts`), which has no
// filesystem: choosing a folder answers with a made-up path and exporting
// records the folder it was asked for and resolves the entry count. Writing
// real files, and refusing the wrong folders, is `src-tauri/src/export.rs`'s
// `cargo test`. What the URL can ask for:
//
//   ?fail=export   the export is refused, with `?failMessage=` as the reason

const FOLDER = "/home/you/Journal export";

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

function dialog(page: Page) {
  return page.getByRole("dialog", { name: "Export journal" });
}

/// Opens the dialog and picks a folder, which is the point at which it names
/// the folder and warns.
async function chooseFolder(page: Page) {
  await page.getByRole("button", { name: "Export journal" }).click();
  await dialog(page).getByRole("button", { name: "Choose folder…" }).click();
  await expect(dialog(page).getByTestId("export-directory")).toHaveText(FOLDER);
}

/// The folders the fallback was asked to export to. Read from the module the
/// app itself loaded — the same URL is the same instance — because an export
/// that ran and one that did not look identical on the page until it finishes.
function exportedTo(page: Page): Promise<string[]> {
  const module = "/src/api/fallback.ts";
  return page.evaluate(
    async (url) => [
      ...(await import(/* @vite-ignore */ url)).exportedDirectories(),
    ],
    module,
  );
}

test("the rail's export button opens a dialog that has written nothing", async ({
  page,
}) => {
  await login(page);
  await page.getByRole("button", { name: "Export journal" }).click();

  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page)).toContainText("markdown file");
  // Nothing to confirm until a folder is chosen: no Export button yet.
  await expect(
    dialog(page).getByRole("button", { name: "Export" }),
  ).toHaveCount(0);
  expect(await exportedTo(page)).toEqual([]);
});

test("the confirmation names the folder and says the files are not encrypted", async ({
  page,
}) => {
  await login(page);
  await chooseFolder(page);

  await expect(dialog(page)).toContainText("Export 2 entries to");
  await expect(dialog(page)).toContainText("not encrypted");
  await expect(dialog(page)).toContainText("read your whole journal");
  expect(await exportedTo(page)).toEqual([]);
});

test("cancelling after choosing a folder writes nothing", async ({ page }) => {
  await login(page);
  await chooseFolder(page);

  await dialog(page).getByRole("button", { name: "Cancel" }).click();

  await expect(dialog(page)).toBeHidden();
  expect(await exportedTo(page)).toEqual([]);

  // Reopening starts over: a folder chosen once is not silently reused.
  await page.getByRole("button", { name: "Export journal" }).click();
  await expect(
    dialog(page).getByRole("button", { name: "Choose folder…" }),
  ).toBeVisible();
  await expect(dialog(page).getByTestId("export-directory")).toHaveCount(0);
});

test("confirming exports every entry to the chosen folder", async ({
  page,
}) => {
  await login(page);
  await chooseFolder(page);

  await dialog(page)
    .getByRole("button", { name: "Export", exact: true })
    .click();

  await expect(dialog(page)).toContainText(`Exported 2 entries to ${FOLDER}.`);
  expect(await exportedTo(page)).toEqual([FOLDER]);

  await dialog(page).getByRole("button", { name: "Close" }).click();
  await expect(dialog(page)).toBeHidden();
});

test("a refused export shows the reason and can be tried again", async ({
  page,
}) => {
  await login(
    page,
    "?fail=export&failMessage=that%20folder%20is%20not%20empty",
  );
  await chooseFolder(page);

  await dialog(page)
    .getByRole("button", { name: "Export", exact: true })
    .click();

  await expect(
    dialog(page).getByRole("alert").filter({ hasText: "not empty" }),
  ).toBeVisible();
  // Still on the confirmation, with the folder and the warning: nothing was
  // reported as exported, and Export is there to try again.
  await expect(dialog(page).getByTestId("export-directory")).toHaveText(FOLDER);
  await expect(dialog(page)).not.toContainText("Exported");
  await expect(
    dialog(page).getByRole("button", { name: "Export", exact: true }),
  ).toBeEnabled();
  expect(await exportedTo(page)).toEqual([]);
});
