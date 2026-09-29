import type { ImageFormat, MediaMeta } from "../types";

/// Every stored image format. A `Record` so a new `ImageFormat` is a compile
/// error here until it is listed.
const IMAGE_FORMATS: Record<ImageFormat, true> = {
  webp: true,
  jpeg: true,
  png: true,
  gif: true,
  heic: true,
  avif: true,
  tiff: true,
};

export function isImageFormat(value: unknown): value is ImageFormat {
  return (
    typeof value === "string" &&
    Object.prototype.hasOwnProperty.call(IMAGE_FORMATS, value)
  );
}

/// The MIME type to give a `Blob` holding this media. Exhaustive on purpose:
/// a new kind or format is a compile error here until it is handled.
export function mediaMime(meta: MediaMeta): string {
  switch (meta.kind) {
    case "image":
      switch (meta.format) {
        case "webp":
          return "image/webp";
        case "jpeg":
          return "image/jpeg";
        case "png":
          return "image/png";
        case "gif":
          return "image/gif";
        case "heic":
          return "image/heic";
        case "avif":
          return "image/avif";
        case "tiff":
          return "image/tiff";
        default: {
          const unhandled: never = meta.format;
          throw new Error(`unhandled image format ${String(unhandled)}`);
        }
      }
    default: {
      const unhandled: never = meta.kind;
      throw new Error(`unhandled media kind ${String(unhandled)}`);
    }
  }
}

/// The stored format for a MIME type `canvas.toBlob` reported, or `null`.
/// Canvas outputs only: a picked file's format comes from `sniffImageFormat`.
export function imageFormatOfMime(mime: string): ImageFormat | null {
  switch (mime) {
    case "image/webp":
      return "webp";
    case "image/jpeg":
      return "jpeg";
    case "image/png":
      return "png";
    case "image/gif":
      return "gif";
    default:
      return null;
  }
}

/// Whether every webview the app ships in can display it. Mirrors
/// `ImageFormat::is_web_safe` in `crates/entry`.
export function isWebSafe(format: ImageFormat): boolean {
  switch (format) {
    case "webp":
    case "jpeg":
    case "png":
    case "gif":
      return true;
    case "heic":
    case "avif":
    case "tiff":
      return false;
    default: {
      const unhandled: never = format;
      throw new Error(`unhandled image format ${String(unhandled)}`);
    }
  }
}

const startsWith = (bytes: Uint8Array, prefix: readonly number[], at = 0) =>
  bytes.length >= at + prefix.length &&
  prefix.every((byte, i) => bytes[at + i] === byte);

const ascii = (text: string) => Array.from(text, (c) => c.charCodeAt(0));

const HEIC_BRANDS = [
  "heic",
  "heix",
  "hevc",
  "hevx",
  "heim",
  "heis",
  "hevm",
  "hevs",
];

const brandOf = (bytes: Uint8Array, at: number) =>
  String.fromCharCode(...bytes.subarray(at, at + 4));

/// The format the bytes are, judged by their header alone, never by a file
/// name or MIME type. `null` for anything else, SVG included. Mirrors
/// `ImageFormat::sniff` in `crates/entry`; keep the two in step.
export function sniffImageFormat(bytes: Uint8Array): ImageFormat | null {
  if (startsWith(bytes, [0xff, 0xd8, 0xff])) return "jpeg";
  if (startsWith(bytes, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))
    return "png";
  if (startsWith(bytes, ascii("GIF87a")) || startsWith(bytes, ascii("GIF89a")))
    return "gif";
  if (startsWith(bytes, ascii("RIFF")) && startsWith(bytes, ascii("WEBP"), 8))
    return "webp";
  if (
    startsWith(bytes, [0x49, 0x49, 0x2a, 0x00]) ||
    startsWith(bytes, [0x4d, 0x4d, 0x00, 0x2a])
  )
    return "tiff";
  if (bytes.length >= 12 && startsWith(bytes, ascii("ftyp"), 4)) {
    const major = brandOf(bytes, 8);
    if (major === "avif" || major === "avis") return "avif";
    if (HEIC_BRANDS.includes(major)) return "heic";
    if (major === "mif1" || major === "msf1") {
      // Compatible brands follow the major brand and its version, up to the
      // box size (bounded by what we have).
      const size = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      ).getUint32(0);
      const end = Math.min(size, bytes.length);
      const brands: string[] = [];
      for (let at = 16; at + 4 <= end; at += 4) brands.push(brandOf(bytes, at));
      if (brands.some((b) => b === "avif" || b === "avis")) return "avif";
      if (brands.some((b) => HEIC_BRANDS.includes(b))) return "heic";
    }
  }
  return null;
}
