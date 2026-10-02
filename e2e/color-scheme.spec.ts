import { test, expect, type Page } from "./coverage";
import { SCHEMES, SCHEME_ORDER, contrastRatio } from "../src/schemes";

// Colour schemes (`src/schemes.ts`): the picker on the Settings page, its
// persistence, how it combines with colour mode, and the contrast of every
// scheme's text colours.

async function login(page: Page) {
  await page.goto("/");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

async function openSettings(page: Page) {
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByTestId("settings-page")).toBeVisible();
}

async function pickScheme(page: Page, label: string) {
  await page.getByTestId("settings-page").getByLabel("Colour scheme").click();
  await page.getByRole("option", { name: label }).click();
}

function bodyBackground(page: Page) {
  return page.evaluate(() => getComputedStyle(document.body).backgroundColor);
}

function toRgb(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgb(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255})`;
}

async function setMode(page: Page, name: "Light" | "Dark") {
  await page
    .getByRole("group", { name: "Colour mode" })
    .getByRole("button", { name })
    .click();
}

test("each scheme paints the page and survives a reload", async ({ page }) => {
  await login(page);
  await openSettings(page);
  await setMode(page, "Light");

  for (const id of SCHEME_ORDER) {
    await pickScheme(page, SCHEMES[id].label);
    await expect
      .poll(() => bodyBackground(page))
      .toBe(toRgb(SCHEMES[id].light.background.default));
  }

  await page.reload();
  await login(page);
  await openSettings(page);
  await expect
    .poll(() => bodyBackground(page))
    .toBe(toRgb(SCHEMES.contrast.light.background.default));
});

test("scheme and colour mode combine", async ({ page }) => {
  await login(page);
  await openSettings(page);
  await setMode(page, "Light");
  await pickScheme(page, "Slate");
  await setMode(page, "Dark");

  await expect
    .poll(() => bodyBackground(page))
    .toBe(toRgb(SCHEMES.slate.dark.background.default));
  await expect(
    page.getByTestId("settings-page").getByLabel("Colour scheme"),
  ).toHaveText("Slate");
});

test("the Ink scheme sets a serif reading font on entries", async ({
  page,
}) => {
  await login(page);
  await openSettings(page);
  await pickScheme(page, "Ink");
  await page.getByRole("button", { name: "Back" }).click();
  await page.getByRole("button", { name: "New entry" }).first().click();

  await expect
    .poll(() =>
      page
        .getByTestId("entry-body")
        .locator(".cm-content")
        .evaluate((el) => getComputedStyle(el).fontFamily),
    )
    .toContain("Georgia");
});

test("an unknown stored scheme falls back to Parchment", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "journal.fallback.device",
      JSON.stringify({ color_mode: "light", color_scheme: "neon" }),
    );
  });
  await login(page);

  await expect
    .poll(() => bodyBackground(page))
    .toBe(toRgb(SCHEMES.parchment.light.background.default));
});

test("every scheme's text colours are readable", () => {
  for (const id of SCHEME_ORDER) {
    for (const mode of ["light", "dark"] as const) {
      const c = SCHEMES[id][mode];
      const body = id === "contrast" ? 7 : 4.5;
      const where = `${id}/${mode}`;
      const opaque = (rgba: string, over: string) => {
        const m = /rgba\((\d+), (\d+), (\d+), ([\d.]+)\)/.exec(rgba);
        if (!m) throw new Error(`not rgba: ${rgba}`);
        const a = Number(m[4]);
        const bg = over
          .slice(1)
          .match(/../g)!
          .map((h) => parseInt(h, 16));
        const ch = [m[1], m[2], m[3]].map(
          (v, i) => Number(v) * a + bg[i] * (1 - a),
        );
        return `#${ch.map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
      };
      const pairs: [string, string, string, number][] = [
        ["text on background", c.text.primary, c.background.default, body],
        ["text on paper", c.text.primary, c.background.paper, body],
        [
          "muted text on paper",
          opaque(c.text.secondary, c.background.paper),
          c.background.paper,
          4.5,
        ],
        ["links on paper", c.primary.main, c.background.paper, 4.5],
        ["on primary", c.primary.contrastText, c.primary.main, 4.5],
        ["on secondary", c.secondary.contrastText, c.secondary.main, 4.5],
        [
          "on primary container",
          c.tonal.onPrimaryContainer,
          c.tonal.primaryContainer,
          4.5,
        ],
        [
          "on secondary container",
          c.tonal.onSecondaryContainer,
          c.tonal.secondaryContainer,
          4.5,
        ],
      ];
      for (const [name, fg, bg, min] of pairs) {
        expect(
          contrastRatio(fg, bg),
          `${where}: ${name} (${fg} on ${bg})`,
        ).toBeGreaterThanOrEqual(min);
      }
    }
  }
});
