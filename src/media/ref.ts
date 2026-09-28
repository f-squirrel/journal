import type { MediaId } from "../types";

// A media reference in markdown: `![alt](media:<entry-id>/<media-id>)`. The
// owning entry is in the reference, so a link pasted into another entry
// still resolves while the owner exists. Mirrors `journal_vault::media_refs`.

const UUID = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
const REF = new RegExp(`^media:(${UUID})/(${UUID})$`, "i");

export interface MediaRefParts {
  entryId: string;
  mediaId: MediaId;
}

export function mediaRef(entryId: string, mediaId: MediaId): string {
  return `media:${entryId}/${mediaId}`;
}

/// The two ids in a `media:` URL, or `null` for anything else.
export function parseMediaRef(src: string): MediaRefParts | null {
  const match = REF.exec(src);
  return match ? { entryId: match[1], mediaId: match[2] } : null;
}

export interface FoundMediaRef extends MediaRefParts {
  /// Offsets of exactly the `media:<e>/<m>` URL in the markdown.
  start: number;
  end: number;
}

/// Every well-formed `](media:<e>/<m>` in `markdown`, in order. The URL ends
/// at the first whitespace or `)`, so a `"title"` after it is fine.
export function findMediaRefs(markdown: string): FoundMediaRef[] {
  const found: FoundMediaRef[] = [];
  const scan = /\]\((media:[^\s)]*)/g;
  for (let m = scan.exec(markdown); m; m = scan.exec(markdown)) {
    const parts = parseMediaRef(m[1]);
    if (!parts) continue;
    const start = m.index + 2;
    found.push({ ...parts, start, end: start + m[1].length });
  }
  return found;
}
