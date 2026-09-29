import { useEffect, useState } from "react";
import { getMedia, MEDIA_NOT_FOUND_MESSAGE } from "../api/media";
import { mediaMime } from "../media/format";
import { parseMediaRef } from "../media/ref";
import type { MediaVariant } from "../types";
import { errorMessage } from "../utils/errors";

/// What an entry's read view shows: the thumbnail, else (an image saved before
/// thumbnails existed) the display copy, else the original.
export const THUMB_CHAIN: readonly MediaVariant[] = [
  "thumb",
  "display",
  "original",
];
/// What the full-size viewer decrypts: a display copy exists exactly when the
/// original cannot be shown everywhere, so the cheap miss falls through.
export const VIEWER_CHAIN: readonly MediaVariant[] = ["display", "original"];

interface Loaded {
  src: string;
  variants: readonly MediaVariant[];
  url?: string;
  error?: string;
}

/// Reads the first variant of `variants` that exists. Only a "no longer
/// available" moves on to the next; any other error is final.
async function readFirst(
  entryId: string,
  mediaId: string,
  variants: readonly MediaVariant[],
) {
  for (const [i, variant] of variants.entries()) {
    try {
      return await getMedia(entryId, mediaId, variant);
    } catch (err) {
      if (
        i === variants.length - 1 ||
        errorMessage(err) !== MEDIA_NOT_FOUND_MESSAGE
      )
        throw err;
    }
  }
  throw MEDIA_NOT_FOUND_MESSAGE;
}

/// Decrypts the image a `media:<entry>/<media>` URL names and gives it an
/// object URL, revoked when the component unmounts or `src` or `variants`
/// change. Pass a module-scope array for `variants`, so its identity is
/// stable. There is deliberately no cache: decrypted bytes live only as long
/// as what shows them, and locking unmounts the panes.
export function useMediaUrl(
  src: string,
  variants: readonly MediaVariant[],
): { url?: string; error?: string } {
  const [loaded, setLoaded] = useState<Loaded | null>(null);

  useEffect(() => {
    const parts = parseMediaRef(src);
    if (!parts) {
      setLoaded({ src, variants, error: "That image link is not valid." });
      return;
    }
    let cancelled = false;
    let url: string | undefined;
    readFirst(parts.entryId, parts.mediaId, variants).then(
      ({ meta, bytes }) => {
        if (cancelled) return;
        url = URL.createObjectURL(
          new Blob([bytes as BlobPart], { type: mediaMime(meta) }),
        );
        setLoaded({ src, variants, url });
      },
      (err) => {
        if (!cancelled) setLoaded({ src, variants, error: errorMessage(err) });
      },
    );
    return () => {
      cancelled = true;
      if (url) URL.revokeObjectURL(url);
    };
  }, [src, variants]);

  // A result for a previous `src` or chain is not this one's.
  return loaded?.src === src && loaded.variants === variants ? loaded : {};
}
