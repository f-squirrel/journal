import { test, expect, type Page } from "./coverage";

// The login screen puts the cursor in the Passphrase field on its own: on
// launch, on every return to it (idle lock, "Log out"), after a failed
// sign-in, and after joining a journal from the cloud. The last two are the
// places a disabled button would otherwise drop focus to the body.

const IDLE_MS = 10_000;

function passphrase(page: Page) {
  return page.getByLabel("Passphrase", { exact: true });
}

async function signIn(page: Page) {
  await passphrase(page).fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
  // Rows are the proof that the listing landed, i.e. the session is fully up.
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
}

/// `install` goes before `goto` so the page's timers are the fake ones from
/// the first script (the idle lock runs on a 10 s timer).
async function login(page: Page, query = "") {
  await page.clock.install();
  await page.goto(`/${query}`);
  await signIn(page);
}

test("the passphrase field has the cursor on launch, with no click", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Sign in to continue")).toBeVisible();
  await expect(passphrase(page)).toBeFocused();

  await page.keyboard.type("typed without clicking");
  await expect(passphrase(page)).toHaveValue("typed without clicking");
});

test("it is focused before the vault check answers, too", async ({ page }) => {
  await page.goto("/?slow=has");
  await expect(passphrase(page)).toBeFocused();
  await expect(page.getByText("Checking your journal…")).toBeVisible();

  await page.keyboard.type("early");
  await expect(passphrase(page)).toHaveValue("early");
});

test("the create form focuses Passphrase, not Confirm", async ({ page }) => {
  await page.goto("/?vault=new");
  await expect(page.getByLabel("Confirm passphrase")).toBeVisible();

  await expect(passphrase(page)).toBeFocused();
});

test("a failed sign-in puts the cursor back, text selected", async ({
  page,
}) => {
  await page.goto("/?fail=unlock&failMessage=Nope.");
  await passphrase(page).fill("wrong");
  await page.getByRole("button", { name: "Sign in" }).click();

  await expect(page.getByRole("alert")).toHaveText("Nope.");
  await expect(page.getByRole("button", { name: "Sign in" })).toBeEnabled();
  await expect(passphrase(page)).toBeFocused();

  // Selected, so the next attempt replaces the wrong one.
  await page.keyboard.type("right");
  await expect(passphrase(page)).toHaveValue("right");
});

test("log out lands on the login screen with the cursor in the field", async ({
  page,
}) => {
  await page.goto("/");
  await signIn(page);

  await page.getByRole("button", { name: "Log out" }).click();

  await expect(passphrase(page)).toBeVisible();
  await expect(passphrase(page)).toBeFocused();
});

test("the idle lock lands with the cursor in the field", async ({ page }) => {
  await login(page);

  await page.clock.runFor(IDLE_MS);

  await expect(passphrase(page)).toBeVisible();
  await expect(passphrase(page)).toBeFocused();
});

/// A modal is on screen when the timer fires; the layout unmounts wholesale,
/// dialog included, so nothing of it may keep focus.
test("the idle lock with the sync dialog open still focuses the field", async ({
  page,
}) => {
  await login(page);
  await page.getByTestId("sync-button").click();
  await expect(page.getByRole("dialog", { name: "Cloud sync" })).toBeVisible();

  await page.clock.runFor(IDLE_MS);

  await expect(passphrase(page)).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(passphrase(page)).toBeFocused();
});

test("joining a journal from the cloud puts the cursor in the field", async ({
  page,
}) => {
  await page.goto("/?vault=new");
  await page.getByRole("button", { name: "Connect Dropbox" }).click();

  // The key arrived with the first pass, so this is a sign-in now. The button
  // that was clicked is gone; without a refocus the cursor would be nowhere.
  await expect(page.getByText("Sign in to continue")).toBeVisible();
  await expect(passphrase(page)).toBeFocused();
});

test("on a phone the focused field is on screen", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto("/");

  await expect(passphrase(page)).toBeFocused();
  await expect(passphrase(page)).toBeInViewport();
  await expect(page.getByRole("button", { name: "Sign in" })).toBeInViewport();
});
