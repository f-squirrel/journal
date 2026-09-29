import { invoke } from "@tauri-apps/api/core";
import { isImageFormat } from "../media/format";
import type { MediaId, MediaMeta, MediaVariant } from "../types";
import { backend, fallback } from "./backend";

// Encrypted media (images) owned by an entry. Same `backend`/`fallback()`
// split as `entries.ts`.

/// The backend's message for media it cannot store; also what a payload we
/// cannot read is reported as.
export const MEDIA_TYPE_MESSAGE =
  "that kind of file cannot be added to an entry";

/// What a read of a missing blob rejects with (`VaultError::MediaNotFound`).
/// `useMediaUrl` falls through its variant chain on exactly this.
export const MEDIA_NOT_FOUND_MESSAGE = "that image is no longer available";

/// The bytes go as the raw request body, not JSON; the ids, the variant and
/// the metadata ride in headers (`media-meta` is `MediaMeta` JSON, which is
/// ASCII).
export async function saveMedia(
  entryId: string,
  mediaId: MediaId,
  variant: MediaVariant,
  meta: MediaMeta,
  bytes: Uint8Array,
): Promise<void> {
  if (backend)
    return invoke("save_media", bytes, {
      headers: {
        "entry-id": entryId,
        "media-id": mediaId,
        "media-variant": variant,
        "media-meta": JSON.stringify(meta),
      },
    });
  return (await fallback()).saveMedia(entryId, mediaId, variant, meta, bytes);
}

/// The payload is `<MediaMeta JSON>\n<bytes>`. Anything that is not an image
/// with a known format is refused rather than trusted.
function decodePayload(buffer: ArrayBuffer): {
  meta: MediaMeta;
  bytes: Uint8Array;
} {
  const all = new Uint8Array(buffer);
  const newline = all.indexOf(0x0a);
  if (newline < 0) throw MEDIA_TYPE_MESSAGE;
  let meta: unknown;
  try {
    meta = JSON.parse(new TextDecoder().decode(all.subarray(0, newline)));
  } catch {
    throw MEDIA_TYPE_MESSAGE;
  }
  const m = meta as Partial<MediaMeta> | null;
  if (m?.kind !== "image" || !isImageFormat(m.format)) throw MEDIA_TYPE_MESSAGE;
  return { meta: m as MediaMeta, bytes: all.subarray(newline + 1) };
}

export async function getMedia(
  entryId: string,
  mediaId: MediaId,
  variant: MediaVariant,
): Promise<{ meta: MediaMeta; bytes: Uint8Array }> {
  if (backend) {
    const payload = await invoke<ArrayBuffer>("get_media", {
      entry_id: entryId,
      media_id: mediaId,
      variant,
    });
    return decodePayload(payload);
  }
  return (await fallback()).getMedia(entryId, mediaId, variant);
}

/// Copies the media `markdown` references into `entryId` and returns the
/// markdown with its links rewritten. Every conflicted copy goes through this,
/// so deleting the original later cannot take the copy's images with it.
export async function adoptMedia(
  entryId: string,
  markdown: string,
): Promise<string> {
  if (backend) return invoke("adopt_media", { entry_id: entryId, markdown });
  return (await fallback()).adoptMedia(entryId, markdown);
}
