import { test, expect, type Page } from "./coverage";

// The calendar is derived from the entry headers the app already holds, so
// these run against the browser fallback like the rest. Two things are pinned
// so they do not depend on the machine: the zone (`entry_date` is a UTC
// instant and a day is the *local* one), and "now", which decides today's
// panel and what "On this day" resurfaces.
//
// The seeded entries, in UTC:
//   "First day back"  — 2026-06-12 07:30
//   "Project ideas"   — 2026-09-02 21:05
//
// `setFixedTime` fixes `Date` and leaves timers running, so the idle lock and
// debounces behave as they do in real life; every test here is far shorter
// than the 10 s lock.

test.use({ timezoneId: "UTC" });

async function login(page: Page, now: string) {
  await page.clock.setFixedTime(new Date(now));
  await page.goto("/");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
}

/// The rail button, by its label — which is also where the "on this day" count
/// lives. Matched loosely so a test can ask for either form.
function railButton(page: Page, name: string | RegExp) {
  // A string is matched whole: the picker has a "calendar view is open…"
  // button of its own, which a substring match would catch.
  return page.getByRole("button", { name, exact: typeof name === "string" });
}

async function openCalendar(page: Page) {
  await railButton(page, /^Calendar/).click();
  await expect(page.getByRole("heading", { name: "Calendar" })).toBeVisible();
}

/// Scoped to the calendar, since the list column has `entry-row`s of its own.
function calendar(page: Page) {
  return page.getByTestId("calendar-page");
}

function day(page: Page, n: number) {
  return calendar(page).getByRole("gridcell", {
    name: String(n),
    exact: true,
  });
}

test("opens from the rail and marks the days that have entries", async ({
  page,
}) => {
  await login(page, "2026-09-21T12:00:00Z");
  await openCalendar(page);

  await expect(calendar(page).getByText("September 2026")).toBeVisible();
  await expect(calendar(page).getByTestId("day-dot")).toHaveCount(1);
  await expect(day(page, 2).getByTestId("day-dot")).toBeVisible();

  for (let i = 0; i < 3; i++) {
    await calendar(page)
      .getByRole("button", { name: "Previous month" })
      .click();
  }
  await expect(calendar(page).getByText("June 2026")).toBeVisible();
  await expect(calendar(page).getByTestId("day-dot")).toHaveCount(1);
  await expect(day(page, 12).getByTestId("day-dot")).toBeVisible();
});

test("tapping a day lists its entries, and a row opens the entry", async ({
  page,
}) => {
  await login(page, "2026-09-21T12:00:00Z");
  await openCalendar(page);

  await day(page, 2).click();
  await expect(
    calendar(page).getByRole("heading", { name: /September 2, 2026/ }),
  ).toBeVisible();
  const rows = calendar(page).getByTestId("entry-row");
  await expect(rows).toHaveCount(1);
  await expect(rows).toContainText("Project ideas");

  await day(page, 3).click();
  await expect(
    calendar(page).getByText("No entries on this day."),
  ).toBeVisible();
  await expect(rows).toHaveCount(0);

  await day(page, 2).click();
  await rows.first().click();
  await expect(page).toHaveURL(/#\/entries\/.+/);
  await expect(page.getByTestId("entry-body")).toContainText("recipe tracker");
});

test("the day panel starts on today", async ({ page }) => {
  await login(page, "2026-09-02T12:00:00Z");
  await openCalendar(page);

  await expect(
    calendar(page).getByRole("heading", { name: /September 2, 2026/ }),
  ).toBeVisible();
  await expect(calendar(page).getByTestId("entry-row")).toContainText(
    "Project ideas",
  );
});

test("a day with two entries lists both", async ({ page }) => {
  await login(page, "2026-09-02T12:00:00Z");

  // Dated the fixed "now", so it lands on the day "Project ideas" is on.
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("Second of the day");
  await openCalendar(page);

  await day(page, 2).click();
  const rows = calendar(page).getByTestId("entry-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.filter({ hasText: "Project ideas" })).toHaveCount(1);
  await expect(rows.filter({ hasText: "Second of the day" })).toHaveCount(1);
});

test("resurfaces earlier years on this day, and the rail says so", async ({
  page,
}) => {
  await login(page, "2027-06-12T12:00:00Z");

  await expect(
    railButton(page, "Calendar — 1 entry on this day"),
  ).toBeVisible();
  await openCalendar(page);

  await expect(
    calendar(page).getByRole("heading", { name: "On this day" }),
  ).toBeVisible();
  await expect(calendar(page).getByText("1 year ago · 2026")).toBeVisible();
  const memory = calendar(page)
    .getByTestId("entry-row")
    .filter({ hasText: "First day back" });
  await expect(memory).toHaveCount(1);

  await memory.click();
  await expect(page).toHaveURL(/#\/entries\/.+/);
  await expect(page.getByTestId("entry-body")).toContainText("Coffee");
});

test("groups memories by year, nearest first", async ({ page }) => {
  await login(page, "2027-06-12T12:00:00Z");

  // Two entries dated the fixed "now", so a year on they share a group. Each
  // is waited for until it is saved, so the next one cannot race its flush.
  for (const title of ["Memory A", "Memory B"]) {
    await page.getByRole("button", { name: "New entry" }).first().click();
    // The previous entry's pane is still mounted until the route switches;
    // typing before that would rename it instead.
    await expect(page.getByPlaceholder("Title")).toHaveValue("");
    await page.getByPlaceholder("Title").fill(title);
    await expect(page.getByTestId("save-status")).toHaveText("Saved");
  }
  await expect(page.getByTestId("entry-row")).toHaveCount(4);

  // A year on. The page reads the clock when it mounts, so opening it now
  // sees the new date; the seeded 2026 entry is two years back by then.
  await page.clock.setFixedTime(new Date("2028-06-12T12:00:00Z"));
  await openCalendar(page);

  const headings = calendar(page).getByText(/years? ago · \d{4}/);
  await expect(headings).toHaveText([
    "1 year ago · 2027",
    "2 years ago · 2026",
  ]);
  const rows = calendar(page).getByTestId("entry-row");
  await expect(rows.filter({ hasText: /Memory [AB]/ })).toHaveCount(2);
  await expect(rows.filter({ hasText: "First day back" })).toHaveCount(1);
});

test("says so when there is nothing to resurface", async ({ page }) => {
  await login(page, "2026-09-21T12:00:00Z");

  await expect(railButton(page, "Calendar")).toBeVisible();
  await openCalendar(page);
  await expect(
    calendar(page).getByText("Nothing from earlier years on this day."),
  ).toBeVisible();
});

test("an entry from this year is not a memory", async ({ page }) => {
  await login(page, "2026-09-02T12:00:00Z");

  // "Project ideas" is dated today, and is in the day panel — but it is not
  // from an earlier year, so the rail stays plain and the section is empty.
  await expect(railButton(page, "Calendar")).toBeVisible();
  await openCalendar(page);
  await expect(calendar(page).getByTestId("entry-row")).toHaveCount(1);
  await expect(
    calendar(page).getByText("Nothing from earlier years on this day."),
  ).toBeVisible();
});

test("on a phone the calendar takes the screen, and Back returns to the list", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page, "2026-09-21T12:00:00Z");

  await page.getByRole("button", { name: "Open menu" }).click();
  await railButton(page, "Calendar").click();

  await expect(page.getByRole("heading", { name: "Calendar" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "All entries" })).toHaveCount(
    0,
  );
  await expect(railButton(page, "Calendar")).toBeHidden();

  const grid = await calendar(page).getByRole("grid").boundingBox();
  expect(grid).not.toBeNull();
  expect(grid!.x).toBeGreaterThanOrEqual(0);
  expect(grid!.x + grid!.width).toBeLessThanOrEqual(390);
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);

  await page.getByRole("button", { name: "Back" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
});

test("logging out from the calendar lands on the sign-in screen", async ({
  page,
}) => {
  await login(page, "2026-09-21T12:00:00Z");
  await openCalendar(page);

  await page.getByRole("button", { name: "Log out" }).click();

  await expect(page.getByLabel("Passphrase")).toBeVisible();
});

test("Today jumps the grid, the selection and the panel back to today", async ({
  page,
}) => {
  // "now" is the day "Project ideas" is on, so the panel's content — not just
  // its heading — says which day is selected.
  await login(page, "2026-09-02T12:00:00Z");
  await openCalendar(page);

  const today = calendar(page).getByRole("button", {
    name: "Today",
    exact: true,
  });
  const previousMonth = calendar(page).getByRole("button", {
    name: "Previous month",
  });
  const selectedDay = calendar(page).getByRole("gridcell", { selected: true });

  // Away in both senses: another day selected, and another month on screen.
  await day(page, 3).click();
  for (let i = 0; i < 3; i++) await previousMonth.click();
  await expect(calendar(page).getByText("June 2026")).toBeVisible();

  await today.click();
  await expect(calendar(page).getByText("September 2026")).toBeVisible();
  await expect(selectedDay).toHaveText("2");
  await expect(
    calendar(page).getByRole("heading", { name: /September 2, 2026/ }),
  ).toBeVisible();
  await expect(calendar(page).getByTestId("entry-row")).toContainText(
    "Project ideas",
  );

  // Today is already the selected day now, so this is the case where the
  // button has nothing to select and only the month is out of place — the one
  // a `value` that did not change identity would silently fail.
  for (let i = 0; i < 3; i++) await previousMonth.click();
  await expect(calendar(page).getByText("June 2026")).toBeVisible();
  await today.click();
  await expect(calendar(page).getByText("September 2026")).toBeVisible();
  await expect(selectedDay).toHaveText("2");
});

test("Today is reachable on a phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page, "2026-09-02T12:00:00Z");

  await page.getByRole("button", { name: "Open menu" }).click();
  await railButton(page, "Calendar").click();
  await expect(page.getByRole("heading", { name: "Calendar" })).toBeVisible();

  // The header is Back, the title and this button; it must not be pushed off
  // the screen by the first two.
  await day(page, 3).click();
  await calendar(page)
    .getByRole("button", { name: "Today", exact: true })
    .click();
  await expect(
    calendar(page).getByRole("heading", { name: /September 2, 2026/ }),
  ).toBeVisible();
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});
