import { test, expect, type Page } from "./coverage";

// The shell is sized in `rem`, so everything in it — the rail, the column
// headers, the list's width, the type inside them — is a multiple of the root
// font size, which is the browser's (and, in a webview, the platform's)
// text-size setting.
//
// These tests turn that setting up and check the shell went with it. Without
// them the `px` values this replaced would come back one `sx` at a time, and
// nothing would notice until someone reading with large text opened the app
// and found a rail clipping its icons.
//
// Measurements are `boundingBox()` (the border box, fractional) rather than
// `clientWidth` (integer, and short by the hairline border, which stays 1px on
// purpose) so the ratios come out exact.

const ROOT = 16;
const LARGE = 24; // The far end of a phone's text-size slider.

async function login(page: Page) {
  await page.goto("/");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

function enlarge(page: Page) {
  return page.addStyleTag({ content: `html { font-size: ${LARGE}px }` });
}

async function fontSize(locator: ReturnType<Page["locator"]>) {
  return locator
    .first()
    .evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
}

/// Every measurement, in one pass, so the two root sizes are compared like for
/// like.
async function shell(page: Page) {
  const header = page.getByRole("heading", { name: "All entries" });
  return {
    railWidth: (await page.getByTestId("sidebar").boundingBox())!.width,
    listWidth: (await page.getByTestId("entry-list").boundingBox())!.width,
    headerHeight: (await header
      .locator("xpath=ancestor::div[1]")
      .boundingBox())!.height,
    headerFont: await fontSize(header),
    rowTitle: await fontSize(
      page.getByTestId("entry-row").first().locator("p").nth(1),
    ),
    search: await fontSize(page.getByLabel("Search entries")),
  };
}

test("the whole shell scales with the browser's text size", async ({
  page,
}) => {
  await login(page);
  const normal = await shell(page);
  await enlarge(page);
  const large = await shell(page);

  const ratio = LARGE / ROOT;
  for (const [what, small] of Object.entries(normal)) {
    expect(
      large[what as keyof typeof large],
      `${what} should scale with the root font size`,
    ).toBeCloseTo(small * ratio, 1);
  }
});

test("the entry list is a column, not a share of the screen", async ({
  page,
}) => {
  await login(page);
  const width = async () =>
    (await page.getByTestId("entry-list").boundingBox())!.width;
  const narrow = await width();

  // A much wider window leaves the column alone: a large monitor should get
  // more room for the entry, not a 600px-wide list of titles. This is the
  // reason nothing in the shell is sized in `vw`.
  await page.setViewportSize({ width: 1800, height: 900 });
  expect(await width()).toBe(narrow);
});
