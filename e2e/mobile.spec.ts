import { test, expect, type Page } from "./coverage";

// UI that only makes sense with a physical keyboard is hidden on phones and
// tablets (`src/platform.ts`). The suite runs desktop Chromium, so each case
// fakes the webview a Tauri iOS/Android build reports — the user agent, or
// for iPadOS in desktop mode the touch-screen "Mac" — before the page loads.

const IPHONE_UA =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
const IPAD_DESKTOP_UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15";

async function login(page: Page) {
  await page.goto("/");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

/// Both places the shortcuts dialog is reachable from, plus its chord.
async function expectNoKeyboardSettings(page: Page) {
  await login(page);

  // Sidebar (a drawer below `md`).
  await page.getByRole("button", { name: "Open menu" }).click();
  const sidebar = page.getByTestId("sidebar");
  await expect(sidebar.getByRole("button", { name: "Settings" })).toBeVisible();
  await expect(
    sidebar.getByRole("button", { name: "Keyboard shortcuts" }),
  ).toHaveCount(0);

  // Settings page.
  await sidebar.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByTestId("settings-page")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Keyboard shortcuts…" }),
  ).toHaveCount(0);

  // A hardware keyboard's chord doesn't open it either.
  await page.keyboard.press("Control+Slash");
  await expect(page.getByRole("dialog")).toHaveCount(0);
}

test.describe("iPhone", () => {
  test.use({
    userAgent: IPHONE_UA,
    viewport: { width: 390, height: 844 },
    hasTouch: true,
  });

  test("hides the keyboard shortcut settings", async ({ page }) => {
    await expectNoKeyboardSettings(page);
  });
});

test.describe("Android", () => {
  test.use({
    userAgent: ANDROID_UA,
    viewport: { width: 412, height: 915 },
    hasTouch: true,
  });

  test("hides the keyboard shortcut settings", async ({ page }) => {
    await expectNoKeyboardSettings(page);
  });
});

test.describe("iPad in desktop mode", () => {
  // Wide enough for the desktop layout, where the sidebar is always mounted.
  test.use({
    userAgent: IPAD_DESKTOP_UA,
    viewport: { width: 1024, height: 768 },
  });

  test("is told apart from a Mac by its touch screen", async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "platform", { get: () => "MacIntel" });
      Object.defineProperty(navigator, "maxTouchPoints", { get: () => 5 });
    });
    await login(page);
    const sidebar = page.getByTestId("sidebar");
    await expect(
      sidebar.getByRole("button", { name: "Settings" }),
    ).toBeVisible();
    await expect(
      sidebar.getByRole("button", { name: "Keyboard shortcuts" }),
    ).toHaveCount(0);
  });
});

test.describe("a Mac without a touch screen", () => {
  test.use({ userAgent: IPAD_DESKTOP_UA });

  test("still shows the keyboard shortcut settings", async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "platform", { get: () => "MacIntel" });
    });
    await login(page);
    await expect(
      page
        .getByTestId("sidebar")
        .getByRole("button", { name: "Keyboard shortcuts" }),
    ).toBeVisible();
  });
});
