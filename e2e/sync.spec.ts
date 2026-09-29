import { test, expect, type Page } from "./coverage";

// The sync UI, over the fallback's simulation of it (`src/api/fallback.ts`):
// the same sequence the real backend produces — a save marks its entry
// pending at once, a pass uploads it, the mark clears — but no network. What
// the URL can ask for:
//
//   ?sync=on / ?sync=conflict   start connected, or connected with a conflict
//                               that unlocking settles
//   ?sync=pull                  the first successful pass rewrites the first
//                               seed entry, as if another device had edited it
//   ?sync=busy                  every pass also fires `sync://changed`
//   ?fail=resolve               a conflict stays parked
//   ?sync=reauth                start with a login the provider refuses
//   ?sync=key_mismatch          start against a cloud holding another journal
//   ?fail=sync                  every pass fails
//   ?fail=connect               connecting is refused
//   ?slow=sync                  a pass starts after 1.2 s, long enough to see
//                               a pending mark, then syncs for 1.2 s more

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

function row(page: Page, title: string) {
  return page.getByTestId("entry-row").filter({ hasText: title });
}

function marks(page: Page) {
  return page.getByTestId("sync-mark");
}

/// The rail's button, whose label is the status itself.
function rail(page: Page) {
  return page.getByTestId("sync-button");
}

/// Asserts what the rail says. Reads `aria-label`, which is what its accessible
/// name is made of, rather than the computed name: while the dialog is open MUI
/// hides everything behind it from the accessibility tree, and the computed
/// name is then empty.
function expectRail(page: Page, label: string | RegExp) {
  return expect(rail(page)).toHaveAttribute("aria-label", label);
}

/// Writes a new entry and waits for it to reach the backend.
async function writeEntry(page: Page, title: string) {
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByTestId("entry-body").locator(".cm-content").fill("Words.");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
}

function section(page: Page) {
  return page.getByTestId("sync-settings");
}

function dropboxRow(page: Page) {
  return page.getByTestId("provider-dropbox");
}

/// Opens Settings › Sync from the rail and returns the section.
async function openSync(page: Page) {
  await rail(page).click();
  const section = page.getByTestId("sync-settings");
  await expect(section).toBeVisible();
  return section;
}

/// Runs a pass with the entry pane still open. The Sync section replaces the
/// pane, so a spec about a pass landing on an open entry cannot press "Sync
/// now": the fallback lets it start one directly.
async function syncWithoutLeaving(page: Page) {
  await page.evaluate(() => window.__syncTestHooks?.syncNow());
}

test("sync is off until an account is connected, and marks nothing", async ({
  page,
}) => {
  await login(page);
  await expectRail(page, "Sync is off");

  await writeEntry(page, "Nobody is listening");

  // Without this, every row would carry a mark.
  await expect(row(page, "Nobody is listening")).toBeVisible();
  await expect(marks(page)).toHaveCount(0);
  await expectRail(page, "Sync is off");
});

test("connecting from the rail turns sync on and pulls what the cloud holds", async ({
  page,
}) => {
  await login(page);
  await expect(page.getByTestId("entry-row")).toHaveCount(2);

  await openSync(page);
  await expect(section(page)).toContainText("Keep a copy of your journal");
  await dropboxRow(page)
    .getByRole("button", { name: "Connect", exact: true })
    .click();

  await expect(section(page)).toContainText("Connected to Dropbox");
  await expectRail(page, /^Synced/);
  // `sync://changed`: the first pass pulled an entry another device wrote, and
  // the list reloaded without being asked.
  await expect(row(page, "Written on the laptop")).toBeVisible();
  await expect(page.getByTestId("entry-row")).toHaveCount(3);
});

test("a refused connection says why and stays off", async ({ page }) => {
  await login(page, "?fail=connect&failMessage=Dropbox%20said%20no");

  await openSync(page);
  await dropboxRow(page)
    .getByRole("button", { name: "Connect", exact: true })
    .click();

  await expect(section(page).getByRole("alert")).toHaveText("Dropbox said no");
  await expectRail(page, "Sync is off");
  // And it can be tried again.
  await expect(
    dropboxRow(page).getByRole("button", { name: "Connect", exact: true }),
  ).toBeEnabled();
});

test("a saved entry is marked until it is uploaded, on its row and on the rail", async ({
  page,
}) => {
  await login(page, "?sync=on&slow=sync");
  await expectRail(page, /^Synced/);

  await writeEntry(page, "On its way");

  const mark = row(page, "On its way").getByRole("img", {
    name: "Not uploaded yet",
  });
  await expect(mark).toBeVisible();
  await expectRail(page, /not uploaded yet/);

  // The pass runs; the mark stays until it has uploaded.
  await expectRail(page, "Syncing…");
  await expect(mark).toBeVisible();

  // The simulated pass lands and both clear.
  await expect(mark).toHaveCount(0);
  await expectRail(page, /^Synced/);
  // Untouched rows never had one.
  await expect(marks(page)).toHaveCount(0);
});

test("a failing pass leaves the mark and says offline", async ({ page }) => {
  await login(page, "?sync=on&fail=sync&failMessage=no%20route%20to%20host");

  await writeEntry(page, "Stuck");

  await expect(
    row(page, "Stuck").getByRole("img", { name: "Not uploaded yet" }),
  ).toBeVisible();
  await expectRail(page, "Offline · 1 change not uploaded");
  // It does not clear itself: nothing reached the cloud.
  await page.waitForTimeout(800);
  await expect(marks(page)).toHaveCount(1);

  await openSync(page);
  await expect(section(page)).toContainText("no route to host");
  await section(page).getByRole("button", { name: "Sync now" }).click();
  // Retrying fails the same way and says so, rather than looking like it worked.
  await expect(section(page)).toContainText("no route to host");
  await expect(marks(page)).toHaveCount(1);
});

test("an entry with conflicting versions is marked as one", async ({
  page,
}) => {
  // Kept parked: unlocking would otherwise settle it at once.
  await login(page, "?sync=conflict&fail=resolve");

  await expect(
    row(page, "First day back").getByRole("img", {
      name: /Conflicting versions/,
    }),
  ).toBeVisible();
  await expect(marks(page)).toHaveCount(1);
  await expectRail(page, "Conflicting versions to resolve");
});

test("unlocking settles a parked conflict into a conflicted copy", async ({
  page,
}) => {
  await login(page, "?sync=conflict");

  // The newer version keeps the entry; the older sits beside it as a copy.
  await expect(row(page, "First day back (conflicted copy)")).toBeVisible();
  await expect(marks(page)).toHaveCount(0);
  await expectRail(page, /^Synced/);
  await expect(page.getByTestId("entry-row")).toHaveCount(3);

  await row(page, "First day back (conflicted copy)").click();
  await expect(page.getByTestId("entry-body")).toContainText(
    "Written on the phone",
  );
});

test("a refused login says so and signing in again recovers", async ({
  page,
}) => {
  await login(page, "?sync=reauth");
  await expectRail(page, "Sign in to Dropbox again");

  // A save while halted waits: it is not uploaded, and nothing retries it.
  await writeEntry(page, "Written meanwhile");
  await expect(marks(page)).toHaveCount(1);
  await expectRail(page, "Sign in to Dropbox again");

  await openSync(page);
  await expect(section(page)).toContainText("reconnect to continue");
  await dropboxRow(page)
    .getByRole("button", { name: "Sign in", exact: true })
    .click();

  await expect(section(page)).not.toContainText("reconnect to continue");
  await expect(
    section(page).getByRole("button", { name: "Sync now" }),
  ).toBeVisible();
  // Its first pass uploads what waited.
  await expect(marks(page)).toHaveCount(0);
  await expectRail(page, /^Synced/);
});

test("a cloud holding another journal says why and offers to disconnect", async ({
  page,
}) => {
  await login(page, "?sync=key_mismatch");
  await expectRail(
    page,
    "This device and the cloud have different journal keys",
  );

  await openSync(page);
  await expect(section(page)).toContainText(
    "the journal key on this device differs from the one in the cloud",
  );
  // Retrying cannot help, and says so rather than looking like it worked.
  await section(page).getByRole("button", { name: "Sync now" }).click();
  await expect(section(page).getByRole("alert")).toHaveCount(2);

  await section(page)
    .getByRole("button", { name: "Disconnect", exact: false })
    .click();
  await section(page)
    .getByRole("button", { name: "Disconnect", exact: false })
    .click();
  await expectRail(page, "Sync is off");
  await expect(page.getByTestId("entry-row")).toHaveCount(2);
});

test("disconnecting warns about what has not been uploaded", async ({
  page,
}) => {
  await login(page, "?sync=on&fail=sync");
  await writeEntry(page, "Unsent");
  await expect(marks(page)).toHaveCount(1);

  await openSync(page);
  await section(page)
    .getByRole("button", { name: "Disconnect", exact: false })
    .click();

  await expect(section(page)).toContainText(
    "1 change has not been uploaded. They stay on this device but will not reach Dropbox.",
  );
  // Backing out changes nothing.
  await section(page).getByRole("button", { name: "Keep syncing" }).click();
  await expectRail(page, /Offline/);

  await section(page)
    .getByRole("button", { name: "Disconnect", exact: false })
    .click();
  await section(page)
    .getByRole("button", { name: "Disconnect", exact: false })
    .click();
  await expect(section(page)).toContainText("Keep a copy of your journal");
  await expectRail(page, "Sync is off");
  // The entry is still here; only its mark is gone.
  await expect(row(page, "Unsent")).toBeVisible();
  await expect(marks(page)).toHaveCount(0);
});

test("disconnecting with nothing pending does not scare anyone", async ({
  page,
}) => {
  await login(page, "?sync=on");

  await openSync(page);
  await section(page)
    .getByRole("button", { name: "Disconnect", exact: false })
    .click();

  await expect(section(page)).toContainText(
    "Your journal on this device is not touched",
  );
  await expect(section(page)).not.toContainText("not been uploaded");
});

test("a new device joins a journal from the cloud before it has a vault", async ({
  page,
}) => {
  await page.goto("/?vault=new");
  await expect(page.getByLabel("Confirm passphrase")).toBeVisible();

  await page.getByRole("button", { name: "Connect Dropbox" }).click();

  // The key arrived with the first pass, so this is a sign-in now, not a create.
  await expect(page.getByText("Sign in to continue")).toBeVisible();
  await expect(page.getByLabel("Confirm passphrase")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Connect Dropbox" }),
  ).toHaveCount(0);

  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(row(page, "Written on the laptop")).toBeVisible();
  await expectRail(page, /^Synced/);
});

test("the login screen offers no cloud connection to someone with a journal", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Sign in to continue")).toBeVisible();

  await expect(
    page.getByRole("button", { name: "Connect Dropbox" }),
  ).toHaveCount(0);
});

test("on a phone the rail lives in the drawer, and a mark costs a row no height", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await login(page, "?sync=on&fail=sync");
  const plain = await row(page, "Project ideas").boundingBox();

  await writeEntry(page, "Phone entry");
  await page.getByRole("button", { name: "Back" }).click();
  const marked = await row(page, "Phone entry").boundingBox();

  await expect(
    row(page, "Phone entry").getByRole("img", { name: "Not uploaded yet" }),
  ).toBeVisible();
  expect(plain).not.toBeNull();
  expect(marked?.height).toBeCloseTo(plain!.height, 0);

  // The rail is behind the menu here, and its button is reachable and named.
  await page.getByRole("button", { name: "Open menu" }).click();
  await expectRail(page, /^Offline/);
  const section = await openSync(page);
  const box = await section.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(390);
});

// A `sync://changed` reload reads the disk once; a save that lands while it is
// in flight is not in what it returns. Replacing the list with that result put
// the row's old title back until the next save. The fake clock pins the order:
// with `?sync=busy` every pass fires `sync://changed` as it ends, `?slow=sync`
// starts a pass 1.2 s after a save and runs it for 1.2 s, and `?slow=list`
// holds the listing it starts open for 1.2 s more. The clock is paused, not
// only installed: time that flows while an assertion waits would let the next
// listing land and hide the race.
test("a list reload that races a save keeps the saved title", async ({
  page,
}) => {
  const start = new Date("2026-09-20T09:00:00Z");
  await page.clock.install({ time: start });
  await page.clock.pauseAt(start);
  await page.goto("/?sync=busy&slow=list,sync");
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.clock.runFor(1200);
  await expect(page.getByTestId("entry-row")).toHaveCount(2);

  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("First title");
  await page.clock.runFor(800);
  await expect(page.getByTestId("save-status")).toHaveText("Saved");

  // The pass after that save ends 2.4 s later and starts a listing, which
  // reads "First title". The next save lands while that listing is open.
  await page.clock.runFor(2400);
  await page.getByPlaceholder("Title").fill("Second title");
  await page.clock.runFor(800);
  await expect(page.getByTestId("save-status")).toHaveText("Saved");

  // The listing lands; the pass after the second save has not started yet.
  await page.clock.runFor(500);

  await expect(row(page, "Second title")).toBeVisible();
  await expect(row(page, "First title")).toHaveCount(0);
});

// GEN-1: an unconditional save used to let the open pane overwrite an edit
// sync had just pulled in. `save_entry` is now conditional on the
// `updated_at` the pane last saw (`Vault::save_entry_if`,
// `VaultError::Stale`), and `?sync=pull` simulates the pull that exposes it —
// see ADR 0012's amendment.

test("a pass that pulls the open entry shows the new text", async ({
  page,
}) => {
  await login(page, "?sync=pull");
  await row(page, "First day back").click();
  await expect(page.getByTestId("entry-body")).toContainText(
    "Started journaling again",
  );

  await syncWithoutLeaving(page);

  // The pane had nothing unsaved, so it refreshed itself on `sync://changed`
  // rather than waiting for a save to meet Stale.
  await expect(page.getByTestId("entry-body")).toContainText(
    "Rewritten on another device.",
  );
});

test("an edit made while a pass pulled the entry is kept as a conflicted copy", async ({
  page,
}) => {
  await page.clock.install();
  await login(page, "?sync=pull");

  await row(page, "First day back").click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  // No time passes: the 800 ms debounce has not fired, so this pane is dirty
  // when the pass below lands.
  await page.getByTestId("entry-body").locator(".cm-content").fill("mine");

  await syncWithoutLeaving(page);

  // The debounce fires: the save meets Stale, and the draft becomes a copy.
  await page.clock.runFor(1000);

  await expect(page.getByTestId("entry-body")).toContainText(
    "Rewritten on another device.",
  );
  await expect(page.getByRole("alert")).toBeVisible();

  const copy = row(page, "(conflicted copy)");
  await expect(copy).toBeVisible();
  await copy.click();
  await expect(page.getByTestId("entry-body")).toContainText("mine");
});

test("a conflicted copy that fails to save can be retried", async ({
  page,
}) => {
  await page.clock.install();
  // The debounced save that meets Stale is the first `save` call and is not
  // itself the injected fault — it fails on its own, as `Vault::save_entry_if`
  // does. `failAfter=1&failTimes=1` bites only the second `save` call, the
  // copy write `useAutosave` makes right after catching Stale.
  await login(page, "?sync=pull&fail=save&failAfter=1&failTimes=1");

  await row(page, "First day back").click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  // No time passes: the 800 ms debounce has not fired, so this pane is dirty
  // when the pass below lands.
  await page.getByTestId("entry-body").locator(".cm-content").fill("mine");

  await syncWithoutLeaving(page);

  // The debounce fires: the save meets Stale, and the conflicted-copy write
  // itself fails once. Dirty stays set, status is "error", and nothing
  // reschedules on its own.
  await page.clock.runFor(1000);

  await expect(page.getByTestId("save-status")).toHaveText("Couldn't save");
  await expect(page.getByRole("alert")).toContainText("Injected fault.");

  // Retry meets Stale again on the original id and retries the copy, which
  // now succeeds.
  await page.getByRole("button", { name: "Retry" }).click();

  await expect(page.getByTestId("entry-body")).toContainText(
    "Rewritten on another device.",
  );
  await expect(page.getByRole("alert")).toBeVisible();

  const retriedCopy = row(page, "(conflicted copy)");
  await expect(retriedCopy).toBeVisible();
  await retriedCopy.click();
  await expect(page.getByTestId("entry-body")).toContainText("mine");
});
