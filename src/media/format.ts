import type { ImageFormat, MediaMeta } from "../types";

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
