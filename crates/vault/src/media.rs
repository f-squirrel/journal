//! Naming, payload format and reference scanning for media blobs: immutable
//! encrypted files (images today) owned by an entry.
//!
//! Kept in its own module, like `snapshot.rs`, because the naming has an
//! invariant worth proving in isolation and the scanner is shared by adoption
//! (this crate) and the plaintext export (`src-tauri`).

use std::ops::Range;

use journal_entry::{Id, MediaId, MediaMeta, MediaVariant};
use journal_store::BlobName;

use crate::owned::{self, OwnedBlob};
use crate::VaultError;

/// The largest payload (raw bytes) `Vault::save_media` accepts per variant.
pub(crate) const MAX_ORIGINAL_BYTES: usize = 50 * 1024 * 1024;
pub(crate) const MAX_DISPLAY_BYTES: usize = 25 * 1024 * 1024;
pub(crate) const MAX_THUMB_BYTES: usize = 1024 * 1024;

pub(crate) fn max_bytes(variant: MediaVariant) -> usize {
    match variant {
        MediaVariant::Original => MAX_ORIGINAL_BYTES,
        MediaVariant::Display => MAX_DISPLAY_BYTES,
        MediaVariant::Thumb => MAX_THUMB_BYTES,
    }
}

const DISPLAY_SUFFIX: &str = ".display";
const THUMB_SUFFIX: &str = ".thumb";

/// The markdown URL scheme prefix a reference starts with.
const REF_PREFIX: &str = "](media:";

/// `<entry-id>.media-<media-id>` (the original), with `.display` or `.thumb`
/// appended for the other variants. A `:` cannot appear in a blob name, so the
/// infix is a plain `.media-`. All variants are one kind so every predicate
/// keyed on it (orphan guard, sync resolver, immutable shortcut, deletion)
/// covers them with no further wiring.
pub(crate) struct MediaBlob;

impl OwnedBlob for MediaBlob {
    const INFIX: &'static str = ".media-";
    type Key = (MediaId, MediaVariant);

    fn tail((media, variant): &(MediaId, MediaVariant)) -> String {
        match variant {
            MediaVariant::Original => media.to_string(),
            MediaVariant::Display => format!("{media}{DISPLAY_SUFFIX}"),
            MediaVariant::Thumb => format!("{media}{THUMB_SUFFIX}"),
        }
    }

    fn parse_tail(tail: &str) -> Option<(MediaId, MediaVariant)> {
        let (id, variant) = if let Some(id) = tail.strip_suffix(DISPLAY_SUFFIX) {
            (id, MediaVariant::Display)
        } else if let Some(id) = tail.strip_suffix(THUMB_SUFFIX) {
            (id, MediaVariant::Thumb)
        } else {
            (tail, MediaVariant::Original)
        };
        Some((id.parse().ok()?, variant))
    }
}

/// Whether `name` is one of an entry's media blobs, of any variant.
///
/// Public because the sync resolver must recognise them (a same-name conflict
/// is two encryptions of the same bytes; keep either, no key needed) and the
/// app declares them immutable to the sync engine.
pub fn is_media(name: &BlobName) -> bool {
    owned::parse::<MediaBlob>(name).is_some()
}

/// A decoded media blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub meta: MediaMeta,
    pub bytes: Vec<u8>,
}

impl Media {
    /// The plaintext payload: metadata line, then the raw bytes. This is what
    /// is encrypted at rest and what `get_media` returns over the bridge.
    pub fn to_payload(&self) -> Vec<u8> {
        encode_payload(&self.meta, &self.bytes)
    }
}

/// `<MediaMeta as compact JSON>\n<raw bytes>`. Compact JSON never contains a
/// raw newline, so the first `\n` always ends the metadata.
pub(crate) fn encode_payload(meta: &MediaMeta, bytes: &[u8]) -> Vec<u8> {
    let mut out =
        serde_json::to_vec(meta).expect("a plain enum of strings and integers always serializes");
    out.push(b'\n');
    out.extend_from_slice(bytes);
    out
}

pub(crate) fn decode_payload(plain: &[u8]) -> Result<Media, VaultError> {
    let split = plain
        .iter()
        .position(|&b| b == b'\n')
        .ok_or(VaultError::MediaType)?;
    let meta = serde_json::from_slice(&plain[..split]).map_err(|_| VaultError::MediaType)?;
    Ok(Media {
        meta,
        bytes: plain[split + 1..].to_vec(),
    })
}

/// One `media:<entry>/<media>` reference found in markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRef {
    /// Covers exactly the `media:<e>/<m>` URL.
    pub range: Range<usize>,
    pub entry: Id,
    pub media: MediaId,
}

/// Finds every well-formed `](media:<entry-id>/<media-id>` reference.
///
/// Hand-written rather than a regex (no dependency). The URL runs up to the
/// first ASCII whitespace or `)`, so `![](media:a/b "title")` still matches.
pub fn media_refs(markdown: &str) -> Vec<MediaRef> {
    let mut refs = Vec::new();
    let mut from = 0;
    while let Some(found) = markdown[from..].find(REF_PREFIX) {
        // Points at `media:` (just after `](`).
        let start = from + found + 2;
        let rest = &markdown[start..];
        let len = rest
            .find(|c: char| c.is_ascii_whitespace() || c == ')')
            .unwrap_or(rest.len());
        let end = start + len;
        if let Some((entry, media)) = markdown[start + "media:".len()..end]
            .split_once('/')
            .and_then(|(e, m)| Some((e.parse::<Id>().ok()?, m.parse::<MediaId>().ok()?)))
        {
            refs.push(MediaRef {
                range: start..end,
                entry,
                media,
            });
        }
        from = end.max(start);
    }
    refs
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use journal_entry::ImageFormat;

    fn meta() -> MediaMeta {
        MediaMeta::Image {
            format: ImageFormat::Webp,
            width: 2,
            height: 3,
        }
    }

    #[test]
    fn parse_inverts_the_naming_and_refuses_other_names() {
        let (e, m) = (Id::new(), MediaId::new());
        for variant in [
            MediaVariant::Original,
            MediaVariant::Display,
            MediaVariant::Thumb,
        ] {
            let name = owned::name::<MediaBlob>(&e, &(m, variant));
            assert_eq!(owned::parse::<MediaBlob>(&name), Some((e, (m, variant))));
            assert!(is_media(&name));
        }

        let other = |s: String| BlobName::try_from(s).unwrap();
        for s in [
            format!("{e}.json"),
            format!("{e}.snap-0000000000000001.json"),
            ".vault-key".to_owned(),
            ".journal-settings".to_owned(),
            format!("{e}.media-not-a-uuid"),
            format!("{e}.media-{}", m.to_string().to_uppercase()),
            format!("{e}.media-{m}.THUMB"),
            format!("{e}.media-{m}.thumbx"),
            format!("{e}.media-{m}.thumb.thumb"),
            format!("{e}.media-{m}.original"),
            format!("{e}.media-{m}."),
        ] {
            assert!(!is_media(&other(s.clone())), "{s}");
        }
    }

    #[test]
    fn payload_round_trips_including_newlines_in_the_bytes() {
        let bytes = b"a\nb\n\nc".to_vec();
        let media = Media {
            meta: meta(),
            bytes: bytes.clone(),
        };
        assert_eq!(media.to_payload(), encode_payload(&meta(), &bytes));
        assert_eq!(decode_payload(&media.to_payload()).unwrap(), media);
    }

    #[test]
    fn decode_payload_rejects_malformed_input() {
        assert!(decode_payload(b"no newline").is_err());
        assert!(decode_payload(b"{bad json\nxx").is_err());
        assert!(decode_payload(b"{\"kind\":\"audio\"}\nxx").is_err());
        assert!(decode_payload(
            b"{\"kind\":\"image\",\"format\":\"svg\",\"width\":1,\"height\":1}\nx"
        )
        .is_err());
    }

    #[test]
    fn media_refs_finds_well_formed_references_only() {
        let (e1, m1, e2, m2) = (Id::new(), MediaId::new(), Id::new(), MediaId::new());
        let md = format!(
            "a ![x](media:{e1}/{m1}) b\n![](media:{e2}/{m2} \"title\")\n\
             ![bad](media:nope/{m1}) ![bad](media:{e1}) [link](media:{e1}/{m2}) ![end](media:{e2}/{m1}"
        );
        let refs = media_refs(&md);
        let got: Vec<_> = refs.iter().map(|r| (r.entry, r.media)).collect();
        assert_eq!(got, [(e1, m1), (e2, m2), (e1, m2), (e2, m1)]);
        for r in &refs {
            assert_eq!(
                &md[r.range.clone()],
                format!("media:{}/{}", r.entry, r.media)
            );
        }
    }
}
