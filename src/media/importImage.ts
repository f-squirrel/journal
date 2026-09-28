import type { ImageFormat, MediaMeta } from "../types";
import { imageFormatOfMime } from "./format";

/// The longest side an imported image keeps, in pixels. Never upscaled.
export const MAX_IMAGE_EDGE = 2048;
/// Larger source files are refused before they are decoded.
export const MAX_SOURCE_BYTES = 50 * 1024 * 1024;
const QUALITY = 0.85;

const CANNOT_DECODE =
  "This image format can't be read here. Try a JPEG or PNG.";

async function encode(
  canvas: HTMLCanvasElement,
  mime: string,
): Promise<{ format: ImageFormat; blob: Blob } | null> {
  const blob = await new Promise<Blob | null>((resolve) =>
    canvas.toBlob(resolve, mime, QUALITY),
  );
  if (!blob) return null;
  // A browser that cannot encode the asked-for type silently returns PNG
  // (WebKit does this for WebP), so trust the blob's type, not the request.
  const format = imageFormatOfMime(blob.type);
  return format ? { format, blob } : null;
}

/// Decodes, downscales and re-encodes an image for storage. Re-encoding drops
/// all EXIF, GPS included: deliberate. An animated GIF keeps its first frame.
/// Rejects with a readable message.
export async function importImage(
  file: File,
): Promise<{ meta: MediaMeta; bytes: Uint8Array }> {
  // SVG is a document that can carry script; it is never accepted.
  if (!file.type.startsWith("image/") || file.type === "image/svg+xml")
    throw "Only pictures can be added to an entry.";
  if (file.size > MAX_SOURCE_BYTES) throw "That image is too large to add.";

  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file, { imageOrientation: "from-image" });
  } catch {
    throw CANNOT_DECODE;
  }
  try {
    const scale = Math.min(
      1,
      MAX_IMAGE_EDGE / Math.max(bitmap.width, bitmap.height),
    );
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const context = canvas.getContext("2d");
    if (!context) throw CANNOT_DECODE;

    let encoded = await (async () => {
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      return encode(canvas, "image/webp");
    })();
    if (encoded?.format !== "webp") {
      // JPEG has no alpha: paint white first, or transparent areas go black.
      context.fillStyle = "#fff";
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      encoded = await encode(canvas, "image/jpeg");
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
    bitmap.close();
  }
}
