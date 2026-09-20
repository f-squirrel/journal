import { test, expect, type Page } from "./coverage";

// Search runs against the browser fallback (`src/api/fallback.ts`), which
// matches titles and bodies the way `Vault::search` does — so what these tests
// prove about the frontend is what the Rust backend will hand it.
//
// The seeded entries, and what is only in their bodies:
//   "First day back"  — "Started journaling again", "Coffee", "Long walk"
//   "Project ideas"   — "a recipe tracker", "Ideas are cheap"

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
}

function field(page: Page) {
  return page.getByLabel("Search entries");
}

function rows(page: Page) {
  return page.getByTestId("entry-row");
}

/// The case a frontend-side filter could never pass: `list_entries` returns
/// headers with no markdown, so matching body text is only possible in the
/// backend. "recipe" appears in one entry's body and in neither title.
test("finds an entry by text its title does not contain", async ({ page }) => {
  await login(page);

  await field(page).fill("recipe");

  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page)).toContainText("Project ideas");
  await expect(page.getByTestId("search-snippet")).toContainText("recipe");
});

test("matches titles too, and ignores case", async ({ page }) => {
  await login(page);

  await field(page).fill("FIRST DAY");

  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page)).toContainText("First day back");
});

test("says so plainly when nothing matches", async ({ page }) => {
  await login(page);

  await field(page).fill("kumquat");

  await expect(rows(page)).toHaveCount(0);
  await expect(page.getByText("Nothing matches “kumquat”.")).toBeVisible();
});

test("clearing the query brings the month sections back", async ({ page }) => {
  await login(page);
  await expect(page.getByText("June 2026")).toBeVisible();

  await field(page).fill("recipe");
  await expect(rows(page)).toHaveCount(1);
  // Results are a flat list: a month header over one match is noise.
  await expect(page.getByText("June 2026")).toHaveCount(0);

  await page.getByRole("button", { name: "Clear search" }).click();

  await expect(rows(page)).toHaveCount(2);
  await expect(page.getByText("June 2026")).toBeVisible();
  await expect(page.getByText("September 2026")).toBeVisible();
});

test("escape clears the query", async ({ page }) => {
  await login(page);
  await field(page).fill("recipe");
  await expect(rows(page)).toHaveCount(1);

  await field(page).press("Escape");

  await expect(field(page)).toHaveValue("");
  await expect(rows(page)).toHaveCount(2);
});

/// Ctrl/Cmd-F has to reach the field from inside the editor, which is where
/// the user is when they want to look something up.
test("ctrl-F focuses the field from anywhere", async ({ page }) => {
  await login(page);
  await page.getByText("First day back").click();
  await expect(page.getByTestId("entry-body")).toBeVisible();

  await page.keyboard.press("ControlOrMeta+f");

  await expect(field(page)).toBeFocused();
  await page.keyboard.type("recipe");
  await expect(rows(page)).toHaveCount(1);
});

test("a result opens the entry it names", async ({ page }) => {
  await login(page);

  await field(page).fill("recipe");
  await rows(page).first().click();

  await expect(page.getByTestId("entry-body")).toContainText("recipe tracker");
});

/// The query outlives opening a result: the list column never unmounts, but
/// the state lives in the context rather than the component for this reason.
test("the query survives opening an entry", async ({ page }) => {
  await login(page);

  await field(page).fill("recipe");
  await rows(page).first().click();

  await expect(field(page)).toHaveValue("recipe");
  await expect(rows(page)).toHaveCount(1);
});

/// A failed search must not read as "nothing matched", and must not cost the
/// user the journal they were looking at.
test("a failed search shows the error and keeps the list", async ({ page }) => {
  await login(page, "?fail=search&failMessage=Search+broke.");

  await field(page).fill("recipe");

  await expect(page.getByText("Search broke.")).toBeVisible();
  await expect(rows(page)).toHaveCount(2);
  await expect(page.getByText("June 2026")).toBeVisible();
});
