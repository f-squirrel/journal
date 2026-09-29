import type { MediaMeta } from "../types";
import { imageFormatOfMime, isWebSafe, sniffImageFormat } from "./format";

/// Larger source files are refused before they are read. Equal to the
/// backend's cap on an original.
export const MAX_SOURCE_BYTES = 50 * 1024 * 1024;
/// Canvas limits: WebKit (iOS especially) draws nothing on a canvas over this
/// area, and no engine takes an edge past 16384.
export const MAX_IMAGE_PIXELS = 16_777_216;
export const MAX_IMAGE_EDGE = 16384;
/// The longest side of a thumbnail, in pixels. Never upscaled.
export const THUMB_EDGE = 640;
export const DISPLAY_QUALITY = 0.92;
export const THUMB_QUALITY = 0.8;
/// Mirrors the backend's cap on a display copy.
export const MAX_DISPLAY_BYTES = 25 * 1024 * 1024;

const RETRY_QUALITY = 0.8;

const NOT_A_PICTURE = "Only pictures can be added to an entry.";
const UNSUPPORTED_FORMAT =
  "This image format isn't supported. Try JPEG, PNG, WebP, GIF or HEIC.";
const CANNOT_DECODE =
  "This image format can't be read here. Try a JPEG or PNG.";
const TOO_LARGE = "That image is too large to add.";

export interface Encoded {
  meta: MediaMeta;
  bytes: Uint8Array;
}

async function encode(
  canvas: HTMLCanvasElement,
  mime: string,
  quality: number,
) {
  const blob = await new Promise<Blob | null>((resolve) =>
    canvas.toBlob(resolve, mime, quality),
  );
  if (!blob) return null;
  // A browser that cannot encode the asked-for type silently returns PNG
  // (WebKit does this for WebP), so trust the blob's type, not the request.
  const format = imageFormatOfMime(blob.type);
  return format ? { format, blob } : null;
}

/// Draws `bitmap` at `width`x`height` and encodes it as WebP, or as JPEG where
/// WebP is unavailable. The canvas is released before returning.
async function drawAndEncode(
  bitmap: ImageBitmap,
  width: number,
  height: number,
  quality: number,
): Promise<Encoded> {
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width));
  canvas.height = Math.max(1, Math.round(height));
  try {
    const context = canvas.getContext("2d");
    if (!context) throw CANNOT_DECODE;
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    let encoded = await encode(canvas, "image/webp", quality);
    if (encoded?.format !== "webp") {
      // JPEG has no alpha: paint white first, or transparent areas go black.
      context.fillStyle = "#fff";
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      encoded = await encode(canvas, "image/jpeg", quality);
    }
    if (!encoded) throw CANNOT_DECODE;
    return {
      meta: {
        kind: "image",
        format: encoded.format,
        width: canvas.width,
        height: canvas.height,
      },
      bytes: new Uint8Array(await encoded.blob.arrayBuffer()),
    };
  } finally {
    canvas.width = canvas.height = 0;
  }
}

/// The message for a file whose bytes are no stored format: a picture the
/// webview can draw (BMP, ICO, JPEG XL) is refused for its format, anything
/// else is not a picture.
async function refusal(file: File): Promise<string> {
  try {
    (await createImageBitmap(file)).close();
    return UNSUPPORTED_FORMAT;
  } catch {
    return NOT_A_PICTURE;
  }
}

/// Prepares a picked file for storage: the file itself as the original
/// (EXIF and all), a thumbnail, and, for a format not every webview can show
/// (HEIC, AVIF, TIFF), a full-resolution display copy. The format comes from
/// the bytes, not the name or MIME type. The importing device has to be able
/// to decode the file. A picture in any other format is refused, not
/// converted: the original is stored byte for byte. Rejects with a readable message.
export async function importImage(file: File): Promise<{
  original: Encoded;
  display: Encoded | null;
  thumb: Encoded;
}> {
  if (file.size > MAX_SOURCE_BYTES) throw TOO_LARGE;
  const source = new Uint8Array(await file.arrayBuffer());
  // SVG is a document that can carry script; it sniffs as nothing and is
  // never accepted.
  const format = sniffImageFormat(source);
  if (!format) throw await refusal(file);

  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file, { imageOrientation: "from-image" });
  } catch {
    throw CANNOT_DECODE;
  }
  try {
    const { width, height } = bitmap;
    const original: Encoded = {
      meta: { kind: "image", format, width, height },
      bytes: source,
    };

    let display: Encoded | null = null;
    if (!isWebSafe(format)) {
      const scale = Math.min(
        1,
        Math.sqrt(MAX_IMAGE_PIXELS / (width * height)),
        MAX_IMAGE_EDGE / Math.max(width, height),
      );
      const w = width * scale;
      const h = height * scale;
      display = await drawAndEncode(bitmap, w, h, DISPLAY_QUALITY);
      if (display.bytes.length > MAX_DISPLAY_BYTES)
        display = await drawAndEncode(bitmap, w, h, RETRY_QUALITY);
      if (display.bytes.length > MAX_DISPLAY_BYTES) throw TOO_LARGE;
    }

    const shrink = Math.min(1, THUMB_EDGE / Math.max(width, height));
    const thumb = await drawAndEncode(
      bitmap,
      width * shrink,
      height * shrink,
      THUMB_QUALITY,
    );
    return { original, display, thumb };
  } finally {
    bitmap.close();
  }
}
