import { test, expect, type Page } from "./coverage";

// Settings › Sync, over the fallback (`src/api/fallback.ts`). `?dropboxKey=none`
// simulates a build with no Dropbox app key.

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

async function openSync(page: Page) {
  await page.getByTestId("sync-button").click();
  const section = page.getByTestId("sync-settings");
  await expect(section).toBeVisible();
  return section;
}

function storedKey(page: Page) {
  return page.evaluate(() => {
    const raw = window.localStorage.getItem("journal.fallback.device");
    return raw ? JSON.parse(raw).sync_providers?.dropbox?.app_key : undefined;
  });
}

test("the Sync section lists Dropbox and what is coming", async ({ page }) => {
  await login(page);
  const section = await openSync(page);

  await expect(
    page
      .getByTestId("provider-dropbox")
      .getByRole("button", { name: "Connect", exact: true }),
  ).toBeVisible();
  for (const id of ["google_drive", "icloud", "webdav"]) {
    await expect(page.getByTestId(`provider-${id}`)).toContainText(
      "Coming soon",
    );
    await expect(
      page.getByTestId(`provider-${id}`).getByRole("button"),
    ).toHaveCount(0);
  }
  await expect(section.getByLabel("Check for sync every")).toBeDisabled();
});

test("connecting shows Connected, then Sync now and a confirmed Disconnect", async ({
  page,
}) => {
  await login(page);
  const section = await openSync(page);

  await page
    .getByTestId("provider-dropbox")
    .getByRole("button", { name: "Connect", exact: true })
    .click();

  await expect(page.getByTestId("provider-dropbox")).toContainText("Connected");
  await expect(section.getByRole("button", { name: "Sync now" })).toBeVisible();

  await section.getByRole("button", { name: "Disconnect…" }).click();
  await section.getByRole("button", { name: "Keep syncing" }).click();
  await expect(page.getByTestId("provider-dropbox")).toContainText("Connected");

  await section.getByRole("button", { name: "Disconnect…" }).click();
  await section
    .getByRole("button", { name: "Disconnect", exact: true })
    .click();
  await expect(page.getByTestId("sync-status")).toContainText("Sync is off");
});

test("the rail icon shows the summary and opens the Sync section", async ({
  page,
}) => {
  await login(page);
  const button = page.getByTestId("sync-button");
  await button.hover();
  await expect(page.getByRole("tooltip")).toHaveText("Sync is off");

  await button.click();
  await expect(page).toHaveURL(/#\/settings$/);
  await expect(page.getByTestId("sync-settings")).toBeInViewport();
});

test("the app key rejects what Dropbox would, and saves what it accepts", async ({
  page,
}) => {
  await login(page);
  const section = await openSync(page);
  const field = section.getByTestId("dropbox-app-key");

  await field.fill("not a key!");
  await field.blur();
  await expect(section.getByText("Letters and digits only.")).toBeVisible();
  expect(await storedKey(page)).toBeFalsy();

  await field.fill("abc123XYZ");
  await field.blur();
  await expect.poll(() => storedKey(page)).toBe("abc123XYZ");
  await expect(section.getByText("Letters and digits only.")).toHaveCount(0);

  await expect(
    section.getByText("Use the same key on every device."),
  ).toBeVisible();
  await expect(section.getByTestId("dropbox-key-help")).toBeVisible();
  await section.getByRole("button", { name: "How to get a key" }).click();
  await expect(section.getByText("http://127.0.0.1:53682/")).toBeVisible();
});

test("the app key cannot change while connected", async ({ page }) => {
  await login(page, "?sync=on");
  const section = await openSync(page);

  await expect(section.getByTestId("dropbox-app-key")).toBeDisabled();
  await expect(
    section.getByText("Disconnect to change the app key."),
  ).toBeVisible();
});

test("a build with no key needs one before Dropbox can connect", async ({
  page,
}) => {
  await login(page, "?dropboxKey=none");
  const section = await openSync(page);
  await expect(page.getByTestId("provider-dropbox")).toContainText(
    "Needs an app key",
  );

  await section.getByTestId("dropbox-app-key").fill("abc123");
  await section.getByTestId("dropbox-app-key").blur();

  await expect(
    page
      .getByTestId("provider-dropbox")
      .getByRole("button", { name: "Connect", exact: true }),
  ).toBeVisible();
});

test("the login screen asks for the key before joining from the cloud", async ({
  page,
}) => {
  await page.goto("/?vault=arrives&dropboxKey=none");
  const connect = page.getByRole("button", { name: "Connect Dropbox" });
  await expect(connect).toBeDisabled();
  const field = page.getByTestId("dropbox-app-key");
  await expect(field).toBeVisible();

  await field.fill("abc123");
  await field.blur();
  await expect(connect).toBeEnabled();
  await connect.click();

  await expect(page.getByText("Sign in to continue")).toBeVisible();
});

test("the Sync section fits a phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page);
  await page.getByRole("button", { name: "Open menu" }).click();
  await openSync(page);

  const overflow = await page.evaluate(
    () =>
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
  const box = await page.getByTestId("dropbox-app-key").boundingBox();
  expect(box!.x + box!.width).toBeLessThanOrEqual(390);
});
