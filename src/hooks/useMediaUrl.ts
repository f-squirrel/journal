import { useEffect, useState } from "react";
import { getMedia } from "../api/media";
import { mediaMime } from "../media/format";
import { parseMediaRef } from "../media/ref";
import { errorMessage } from "../utils/errors";

interface Loaded {
  src: string;
  url?: string;
  error?: string;
}

/// Decrypts the image a `media:<entry>/<media>` URL names and gives it an
/// object URL, revoked when the component unmounts or `src` changes. There is
/// deliberately no cache: decrypted bytes live only as long as what shows
/// them, and locking unmounts the panes.
export function useMediaUrl(src: string): { url?: string; error?: string } {
  const [loaded, setLoaded] = useState<Loaded | null>(null);

  useEffect(() => {
    const parts = parseMediaRef(src);
    if (!parts) {
      setLoaded({ src, error: "That image link is not valid." });
      return;
    }
    let cancelled = false;
    let url: string | undefined;
    getMedia(parts.entryId, parts.mediaId).then(
      ({ meta, bytes }) => {
        if (cancelled) return;
        url = URL.createObjectURL(
          new Blob([bytes as BlobPart], { type: mediaMime(meta) }),
        );
        setLoaded({ src, url });
      },
      (err) => {
        if (!cancelled) setLoaded({ src, error: errorMessage(err) });
      },
    );
    return () => {
      cancelled = true;
      if (url) URL.revokeObjectURL(url);
    };
  }, [src]);

  // A result for a previous `src` is not this one's.
  return loaded?.src === src ? loaded : {};
}
