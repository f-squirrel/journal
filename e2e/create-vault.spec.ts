import { test, expect, type Page } from "./coverage";

// The create-vault form is only reachable when there is no vault yet, which
// never happens under the browser fallback on its own — so every case here
// goes through `?vault=new` (see the fault-injection block in
// `src/api/entries.ts`). The app uses HashRouter, so the query string goes
// before the hash.

/// The minimum, read off the field's own helper text rather than duplicated
/// here: the number is the app's to state, and a copy of it in the suite is a
/// second source of truth that can drift.
async function minimumLength(page: Page): Promise<number> {
  const helper = await page.getByText(/at least \d+ characters/i).innerText();
  const match = helper.match(/\d+/);
  expect(match, `no minimum stated in "${helper}"`).not.toBeNull();
  return Number(match![0]);
}

test("the create-vault form asks for confirmation", async ({ page }) => {
  await page.goto("/?vault=new");

  await expect(page.getByLabel("Passphrase", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Confirm passphrase")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Create vault" }),
  ).toBeVisible();
});

test("a mismatched confirmation blocks submit", async ({ page }) => {
  await page.goto("/?vault=new");

  await page
    .getByLabel("Passphrase", { exact: true })
    .fill("correct horse battery");
  await page.getByLabel("Confirm passphrase").fill("correct horse bettery");
  await page.getByRole("button", { name: "Create vault" }).click();

  await expect(
    page.getByText("The two passphrases don't match."),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "All entries" })).toHaveCount(
    0,
  );
});

test("a too-short passphrase blocks submit", async ({ page }) => {
  await page.goto("/?vault=new");

  const short = "x".repeat((await minimumLength(page)) - 1);
  await page.getByLabel("Passphrase", { exact: true }).fill(short);
  await page.getByLabel("Confirm passphrase").fill(short);
  await page.getByRole("button", { name: "Create vault" }).click();

  await expect(
    page.getByRole("alert").filter({ hasText: /at least \d+ characters/i }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "All entries" })).toHaveCount(
    0,
  );
});

test("a matching pair creates the vault", async ({ page }) => {
  await page.goto("/?vault=new");

  const long = "x".repeat(await minimumLength(page));
  await page.getByLabel("Passphrase", { exact: true }).fill(long);
  await page.getByLabel("Confirm passphrase").fill(long);
  await page.getByRole("button", { name: "Create vault" }).click();

  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
});

// The two cases below are about the window *before* `has_vault` answers, when
// the page does not yet know which of the two forms it is. `?slow=has` and
// `?fail=has` drive the fallback's `hasVault` (see R0-1): a form that is live
// during that window takes the sign-in path, skipping the confirmation and the
// minimum-length check, and creates a vault sealed with whatever was typed.

test("the form cannot be submitted before the vault check answers", async ({
  page,
}) => {
  await page.goto("/?vault=new&slow=has");

  await page.getByLabel("Passphrase", { exact: true }).fill("x");

  // Named rather than bare: a failed check renders a second button ("Try
  // again") beside the submit one, and past the login screen there are several.
  await expect(page.getByRole("button", { name: "Checking" })).toBeDisabled();
  await expect(page.getByRole("heading", { name: "All entries" })).toHaveCount(
    0,
  );
});

test("a failed vault check does not create a vault", async ({ page }) => {
  await page.goto("/?vault=new&fail=has");

  // A rejection leaves the answer unknown until the retry succeeds, so the
  // guards must stay on: pressing Enter is how a password manager submits.
  await page.getByLabel("Passphrase", { exact: true }).fill("x");
  await page.getByLabel("Passphrase", { exact: true }).press("Enter");

  await expect(page.getByRole("heading", { name: "All entries" })).toHaveCount(
    0,
  );
  await expect(page.getByRole("button", { name: "Checking" })).toBeDisabled();
});

test("an unanswered vault check blocks a programmatic submit", async ({
  page,
}) => {
  await page.goto("/?vault=new&fail=has");

  // `requestSubmit()` ignores the disabled submit button, which is how an
  // engine that treats a disabled default button as absent behaves on Enter.
  await page.getByLabel("Passphrase", { exact: true }).fill("x".repeat(20));
  await page.evaluate(() => document.querySelector("form")?.requestSubmit());

  await expect(page.getByRole("heading", { name: "All entries" })).toHaveCount(
    0,
  );
  // Not just "no vault": the confirm field was never rendered, so without the
  // guard a returning user is told their passphrases don't match.
  await expect(page.getByText("The two passphrases don't match.")).toHaveCount(
    0,
  );
});

// A journal arriving from the cloud mid-create turns the form into a sign-in.
// Left as a create form it would send the next attempt, with the new
// passphrase, at a key sealed under the other device's — and "Incorrect
// passphrase." would read as the app having mangled what was just chosen.
test("a journal that arrives while creating turns the form into a sign-in", async ({
  page,
}) => {
  await page.goto("/?vault=arrives");
  const passphrase = "a-brand-new-passphrase";
  await page.getByLabel("Passphrase", { exact: true }).fill(passphrase);
  await page.getByLabel("Confirm passphrase").fill(passphrase);
  await page.getByRole("button", { name: "Create vault" }).click();

  await expect(page.getByText(/journal already exists/i)).toBeVisible();
  await expect(page.getByLabel("Confirm passphrase")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
});
