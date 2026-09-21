import { test, expect, type Page } from "./coverage";

// A link in an entry must never navigate the app's own window: it has no
// address bar, so a page loaded into it could pass for the login screen. In
// Tauri the click goes to the opener plugin; here, on the fallback branch, it
// goes to `window.open`, which is what these tests watch.

async function login(page: Page) {
  await page.goto("/");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

test("a link in an entry opens outside the app window", async ({ page }) => {
  await page.addInitScript(() => {
    const opened: unknown[][] = [];
    (window as unknown as { opened: unknown[][] }).opened = opened;
    window.open = (...args: unknown[]) => {
      opened.push(args);
      return null;
    };
  });
  await login(page);

  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("Has a link");
  await page
    .getByTestId("entry-body")
    .locator(".cm-content")
    .fill("[Open my notes](https://evil.example)");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");

  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Preview" }).click();
  const before = page.url();
  await page.getByRole("link", { name: "Open my notes" }).click();

  expect(
    await page.evaluate(
      () => (window as unknown as { opened: unknown[][] }).opened,
    ),
  ).toEqual([["https://evil.example", "_blank", "noopener,noreferrer"]]);
  expect(page.url()).toBe(before);
  await expect(page.getByTestId("entry-body")).toBeVisible();
});
