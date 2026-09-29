import { test, expect, type Page } from "./coverage";

// Encrypted images, entirely against the fallback (`src/api/fallback.ts`),
// which keeps media in a Map and mirrors the vault's rules. The fixture is a
// 1x1 PNG built from a constant, so no binary file is committed.

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

async function login(page: Page, query = "") {
  await page.goto(`/${query}`);
  await page.getByLabel("Passphrase").fill("password123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(
    page.getByRole("heading", { name: "All entries" }),
  ).toBeVisible();
}

function editor(page: Page) {
  return page.getByTestId("entry-body").locator(".cm-content");
}

async function newEntry(page: Page, title: string, body = "") {
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill(title);
  if (body) await editor(page).fill(body);
}

async function pickImage(
  page: Page,
  mimeType = "image/png",
  name = "a.png",
  buffer: Buffer = PNG,
) {
  await page
    .getByTestId("image-input")
    .setInputFiles({ name, mimeType, buffer });
}

async function preview(page: Page) {
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Preview" }).click();
}

const LINK = /!\[\]\(media:[0-9a-f-]{36}\/[0-9a-f-]{36}\)/;

test("Insert image adds a media link to the text", async ({ page }) => {
  await login(page);
  await newEntry(page, "Pics", "hello");
  await pickImage(page);
  await expect(editor(page)).toContainText("](media:");
  await expect(editor(page)).toContainText(LINK);
});

test("pasting an image file adds a link", async ({ page }) => {
  await login(page);
  await newEntry(page, "Pasted");
  await editor(page).click();
  await editor(page).evaluate(
    (el, bytes) => {
      const data = new DataTransfer();
      data.items.add(
        new File([new Uint8Array(bytes)], "p.png", { type: "image/png" }),
      );
      el.dispatchEvent(
        new ClipboardEvent("paste", {
          clipboardData: data,
          bubbles: true,
          cancelable: true,
        }),
      );
    },
    [...PNG],
  );
  await expect(editor(page)).toContainText(LINK);
});

test("a failed save shows the error and inserts no link", async ({ page }) => {
  await login(page, "?fail=save-media");
  await newEntry(page, "Fails", "text");
  await pickImage(page);
  await expect(page.getByTestId("media-error")).toBeVisible();
  await expect(editor(page)).not.toContainText("media:");
});

test("a file that is not an image is refused", async ({ page }) => {
  await login(page);
  await newEntry(page, "Refused", "text");
  await pickImage(page, "text/plain", "a.txt", Buffer.from("just text"));
  await expect(page.getByTestId("media-error")).toContainText("pictures");
  await expect(editor(page)).not.toContainText("media:");
});

test("a file is judged by its bytes, not its name or type", async ({
  page,
}) => {
  await login(page);
  await newEntry(page, "Mislabelled", "text");
  await pickImage(page, "image/png", "a.png", Buffer.from("<svg/>"));
  await expect(page.getByTestId("media-error")).toContainText("pictures");
  await expect(editor(page)).not.toContainText("media:");
});

test("a failed thumbnail save inserts no link", async ({ page }) => {
  // The original saves (call 0); the thumbnail (call 1) fails.
  await login(page, "?fail=save-media&failAfter=1");
  await newEntry(page, "Half", "text");
  await pickImage(page);
  await expect(page.getByTestId("media-error")).toBeVisible();
  await expect(editor(page)).not.toContainText("media:");
});

test("the stored original is the picked file, byte for byte", async ({
  page,
}) => {
  await login(page);
  await newEntry(page, "Exact", "text");
  await pickImage(page);
  await expect(editor(page)).toContainText(LINK);
  const link = (await editor(page).innerText()).match(LINK)![0];
  const [, entryId, mediaId] = link.match(
    /media:([0-9a-f-]{36})\/([0-9a-f-]{36})/,
  )!;
  const stored = await page.evaluate(
    async ([entryId, mediaId]) => {
      const { getMedia } = await import(/* @vite-ignore */ "/src/api/media.ts");
      const original = await getMedia(entryId, mediaId, "original");
      const thumb = await getMedia(entryId, mediaId, "thumb");
      return {
        original: [...original.bytes],
        originalMeta: original.meta,
        thumbFormat: thumb.meta.kind === "image" ? thumb.meta.format : null,
      };
    },
    [entryId, mediaId],
  );
  expect(stored.original).toEqual([...PNG]);
  expect(stored.originalMeta).toMatchObject({ format: "png", width: 1 });
  expect(["webp", "jpeg"]).toContain(stored.thumbFormat);
});

test("text typed while an image saves stays, and the link lands where the paste was", async ({
  page,
}) => {
  // Two saves (original, thumbnail), so ~800 ms in all.
  await login(page, "?slow=save-media&slowMs=400");
  await newEntry(page, "Slow", "first");
  await pickImage(page);
  // The button inserts at the cursor as it is when the save finishes, so a
  // paste is what pins the position.
  await editor(page).press("Home");
  await editor(page).pressSequentially("ZZ");
  await expect(editor(page)).toContainText(LINK);
  // Typing kept its place, and the link went on a line of its own.
  await expect
    .poll(() => editor(page).innerText())
    .toMatch(/^ZZ\n!\[\]\(media:[0-9a-f-]{36}\/[0-9a-f-]{36}\)\nfirst$/);
});

test("switching entries while an image saves inserts nothing anywhere", async ({
  page,
}) => {
  await login(page, "?slow=save-media&slowMs=500");
  await newEntry(page, "Leaving", "stay");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  await pickImage(page);
  await page
    .getByTestId("entry-row")
    .filter({ hasText: "First day back" })
    .click();
  await page.waitForTimeout(1500);
  await expect(page.getByTestId("entry-body")).not.toContainText("media:");
  await page.getByTestId("entry-row").filter({ hasText: "Leaving" }).click();
  await expect(page.getByTestId("entry-body")).not.toContainText("media:");
});

async function entryWithImage(page: Page, title: string) {
  await newEntry(page, title, "look");
  await pickImage(page);
  await expect(editor(page)).toContainText(LINK);
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
}

test("the read view shows the thumbnail from a blob URL", async ({ page }) => {
  await login(page);
  await entryWithImage(page, "Shown");
  await preview(page);
  const image = page.getByTestId("media-thumb").getByTestId("media-image");
  await expect(image).toBeVisible();
  await expect(image).toHaveAttribute("src", /^blob:/);
});

async function openViewer(page: Page) {
  await page.getByTestId("media-thumb").click();
  await expect(page.getByTestId("image-viewer")).toBeVisible();
}

test("clicking the thumbnail opens a viewer that toggles fit and actual size", async ({
  page,
}) => {
  await login(page);
  await entryWithImage(page, "Viewed");
  await preview(page);
  await openViewer(page);
  const image = page.getByTestId("viewer-image");
  await expect(image).toHaveAttribute("src", /^blob:/);
  await expect(image).toHaveAttribute("data-zoom", "fit");
  await image.click();
  await expect(image).toHaveAttribute("data-zoom", "actual");
  await image.click();
  await expect(image).toHaveAttribute("data-zoom", "fit");
});

test("Escape and the close button close the viewer, leaving the thumbnail in place", async ({
  page,
}) => {
  await login(page);
  await entryWithImage(page, "Closing");
  await preview(page);
  const thumb = page.getByTestId("media-image");
  await thumb.evaluate((el) => {
    (window as unknown as { keep: Element }).keep = el;
  });
  await openViewer(page);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("image-viewer")).toHaveCount(0);
  await openViewer(page);
  await page.getByRole("button", { name: "Close image" }).click();
  await expect(page.getByTestId("image-viewer")).toHaveCount(0);
  const same = await thumb.evaluate(
    (el) => (window as unknown as { keep: Element }).keep === el,
  );
  expect(same).toBe(true);
});

test("the viewer shows a spinner while the full image decrypts", async ({
  page,
}) => {
  await login(page, "?slow=get-media&slowMs=800");
  await entryWithImage(page, "Slow view");
  await preview(page);
  await openViewer(page);
  await expect(page.getByTestId("viewer-loading")).toBeVisible();
  await expect(page.getByTestId("viewer-preview")).toBeVisible();
  await expect(page.getByTestId("viewer-image")).toBeVisible();
  await expect(page.getByTestId("viewer-loading")).toHaveCount(0);
});

test("when the full image cannot be read the thumbnail stays and the viewer says so", async ({
  page,
}) => {
  await login(page, "?fail=get-media");
  await entryWithImage(page, "Half broken");
  await preview(page);
  await expect(page.getByTestId("media-image")).toBeVisible();
  await openViewer(page);
  await expect(page.getByTestId("viewer-error")).toBeVisible();
  await expect(page.getByTestId("viewer-preview")).toBeVisible();
});

test("an image saved without a thumbnail previews from the original", async ({
  page,
}) => {
  await login(page, "?thumbs=off");
  await entryWithImage(page, "Old image");
  await preview(page);
  await expect(page.getByTestId("media-image")).toHaveAttribute(
    "src",
    /^blob:/,
  );
  await openViewer(page);
  await expect(page.getByTestId("viewer-image")).toBeVisible();
});

test("no global shortcut fires while the viewer is open", async ({ page }) => {
  await login(page);
  await entryWithImage(page, "Quiet");
  await preview(page);
  await openViewer(page);
  // "Lock journal" is a global shortcut; it must not act under the viewer.
  await page.keyboard.press("ControlOrMeta+KeyL");
  await expect(page.getByTestId("image-viewer")).toBeVisible();
  await expect(page.getByLabel("Passphrase")).toHaveCount(0);
});

test("opening the viewer raises no DOM-nesting warning", async ({ page }) => {
  const warnings: string[] = [];
  page.on("console", (m) => warnings.push(m.text()));
  await login(page);
  await entryWithImage(page, "Nesting");
  await preview(page);
  await openViewer(page);
  expect(warnings.filter((w) => /validateDOMNesting/.test(w))).toEqual([]);
});

test("an image stays the same DOM node across an unrelated re-render", async ({
  page,
}) => {
  await login(page);
  await entryWithImage(page, "Stable");
  await preview(page);
  const image = page.getByTestId("media-image");
  await expect(image).toBeVisible();
  await image.evaluate((el) => {
    (window as unknown as { keep: Element }).keep = el;
  });
  // Any re-render of the pane: opening and closing the actions menu.
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  const same = await image.evaluate(
    (el) => (window as unknown as { keep: Element }).keep === el,
  );
  expect(same).toBe(true);
});

test("an image that cannot be read shows the unavailable box", async ({
  page,
}) => {
  await login(page, "?fail=get-thumb");
  await entryWithImage(page, "Broken");
  await preview(page);
  // A generic error does not fall through to the next variant.
  await expect(page.getByTestId("media-unavailable")).toBeVisible();
});

test("a typed image path shows a hint and no image", async ({ page }) => {
  await login(page);
  await newEntry(page, "Path", "![x](/home/u/a.jpg)");
  await preview(page);
  await expect(page.getByTestId("image-hint")).toContainText(
    "Use Insert image to add pictures",
  );
  await expect(page.getByTestId("entry-body").locator("img")).toHaveCount(0);
});

test("a media link that is not an image renders as inert text", async ({
  page,
}) => {
  const warnings: string[] = [];
  page.on("console", (m) => warnings.push(m.text()));
  await login(page);
  const id = "11111111-1111-4111-8111-111111111111";
  await newEntry(page, "Link", `[x](media:${id}/${id})`);
  await preview(page);
  await expect(page.getByTestId("entry-body").locator("a")).not.toHaveAttribute(
    "href",
    /media:/,
  );
  expect(warnings.filter((w) => /validateDOMNesting/.test(w))).toEqual([]);
});

test("the history preview renders the image", async ({ page }) => {
  await login(page);
  await entryWithImage(page, "Hist");
  await editor(page).press("End");
  await editor(page).pressSequentially(" more");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Version history" }).click();
  const dialog = page.getByRole("dialog", { name: "Version history" });
  await dialog.getByText("Hist", { exact: true }).first().click();
  await expect(dialog.getByTestId("media-image")).toBeVisible();
  await dialog.getByTestId("media-thumb").click();
  await expect(page.getByTestId("image-viewer")).toBeVisible();
});

test("an image whose entry was deleted is unavailable", async ({ page }) => {
  await login(page);
  await entryWithImage(page, "Owner");
  const link = (await editor(page).innerText()).match(LINK)![0];
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Delete" }).last().click();
  await page.getByRole("button", { name: "New entry" }).first().click();
  await page.getByPlaceholder("Title").fill("Borrower");
  await editor(page).fill(link);
  await preview(page);
  await expect(page.getByTestId("media-unavailable")).toBeVisible();
});

test("a stale-save conflicted copy owns its own copy of the image", async ({
  page,
}) => {
  await page.clock.install();
  await login(page, "?sync=pull");
  await page
    .getByTestId("entry-row")
    .filter({ hasText: "First day back" })
    .click();
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await editor(page).fill("mine");
  await pickImage(page);
  await expect(editor(page)).toContainText(LINK);

  // A sync pass rewrites the entry under the open pane; the debounced save
  // then meets Stale and the draft becomes a copy.
  await page.getByTestId("sync-button").click();
  const cloud = page.getByRole("dialog", { name: "Cloud sync" });
  await cloud.getByRole("button", { name: "Sync now" }).click();
  await cloud.getByRole("button", { name: "Close" }).click();
  await page.clock.runFor(1000);

  const copy = page
    .getByTestId("entry-row")
    .filter({ hasText: "(conflicted copy)" });
  await expect(copy).toBeVisible();
  await copy.click();
  await expect(page.getByTestId("media-image")).toBeVisible();
  const href = page.url().match(/entries\/([0-9a-f-]{36})/)?.[1];
  await page.getByRole("button", { name: "Entry actions" }).click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  // The link now names the copy, not the entry it was made from.
  await expect(editor(page)).toContainText(`media:${href}/`);
});

test("the viewer toggles from the keyboard, and says so when the webview cannot decode the file", async ({
  page,
}) => {
  await login(page);
  await newEntry(page, "Undecodable", "look");
  await expect(page.getByTestId("save-status")).toHaveText("Saved");
  const id = page.url().match(/entries\/([0-9a-f-]{36})/)![1];
  const media = "22222222-2222-4222-8222-222222222222";
  // A PNG signature and nothing else: it sniffs as PNG but cannot be drawn.
  await page.evaluate(
    async ([id, media, png]) => {
      const { saveMedia } = await import(
        /* @vite-ignore */ "/src/api/media.ts"
      );
      const meta = {
        kind: "image",
        format: "png",
        width: 1,
        height: 1,
      } as const;
      const sig = new Uint8Array([
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
      ]);
      await saveMedia(id, media, "original", meta, sig);
      await saveMedia(id, media, "thumb", meta, new Uint8Array(png));
    },
    [id, media, [...PNG]],
  );
  await editor(page).fill(`![](media:${id}/${media})`);
  await preview(page);
  await openViewer(page);
  await expect(page.getByTestId("viewer-error")).toBeVisible();
});

test("the viewer image toggles with Enter", async ({ page }) => {
  await login(page);
  await entryWithImage(page, "Keys");
  await preview(page);
  await openViewer(page);
  const image = page.getByTestId("viewer-image");
  await image.focus();
  await page.keyboard.press("Enter");
  await expect(image).toHaveAttribute("data-zoom", "actual");
  await page.keyboard.press(" ");
  await expect(image).toHaveAttribute("data-zoom", "fit");
});

test("a malformed media link is unavailable", async ({ page }) => {
  await login(page);
  await newEntry(page, "Bad link", "![](media:nope)");
  await preview(page);
  await expect(page.getByTestId("media-unavailable")).toBeVisible();
});

test("the format helpers sniff each format from its header", async ({
  page,
}) => {
  await login(page);
  const got = await page.evaluate(async () => {
    const f = await import(/* @vite-ignore */ "/src/media/format.ts");
    const b = (...xs: (number | string)[]) =>
      new Uint8Array(
        xs.flatMap((x) =>
          typeof x === "string" ? [...x].map((c) => c.charCodeAt(0)) : [x],
        ),
      );
    const ftyp = (major: string, ...compat: string[]) =>
      b(0, 0, 0, 16 + 4 * compat.length, "ftyp", major, 0, 0, 0, 0, ...compat);
    const cases = [
      b(0xff, 0xd8, 0xff, 0xe0),
      b(0x89, "PNG", 0x0d, 0x0a, 0x1a, 0x0a),
      b("GIF89a"),
      b("RIFF", 0, 0, 0, 0, "WEBP"),
      b("II*", 0),
      b("MM", 0, "*"),
      ftyp("avif"),
      ftyp("heic"),
      ftyp("mif1", "miaf", "avif"),
      ftyp("mif1", "miaf", "heic"),
      ftyp("mif1", "miaf"),
      b("<svg/>"),
      b(),
    ];
    const metaFor = (format: string) => ({ kind: "image", format }) as never;
    const formats = ["webp", "jpeg", "png", "gif", "heic", "avif", "tiff"];
    return {
      sniffed: cases.map((c) => f.sniffImageFormat(c)),
      safe: formats.map((x) => f.isWebSafe(x as never)),
      mimes: formats.map((x) => f.mediaMime(metaFor(x))),
      known: [
        f.isImageFormat("heic"),
        f.isImageFormat("svg"),
        f.isImageFormat(3),
      ],
    };
  });
  expect(got.sniffed).toEqual([
    "jpeg",
    "png",
    "gif",
    "webp",
    "tiff",
    "tiff",
    "avif",
    "heic",
    "avif",
    "heic",
    null,
    null,
    null,
  ]);
  expect(got.safe).toEqual([true, true, true, true, false, false, false]);
  expect(got.mimes).toEqual([
    "image/webp",
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/heic",
    "image/avif",
    "image/tiff",
  ]);
  expect(got.known).toEqual([true, false, false]);
});

test("a source file over the size cap is refused before it is read", async ({
  page,
}) => {
  await login(page);
  const message = await page.evaluate(async () => {
    const { importImage } = await import(
      /* @vite-ignore */ "/src/media/importImage.ts"
    );
    const big = new File([new Uint8Array(51 * 1024 * 1024)], "big.jpg");
    return importImage(big).then(
      () => "accepted",
      (e) => String(e),
    );
  });
  expect(message).toContain("too large");
});
