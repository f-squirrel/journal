//! Golden-file tests: an accidental change to `Entry`'s or `Manifest`'s
//! on-disk shape must fail this test loudly, rather than migrate silently
//! (workspace security conventions; ADR-0016). Fixtures live under
//! `tests/golden/` and are fixed, deterministic instances — fixed
//! `EntryId`s, fixed `ObjectName` bytes, fixed timestamps — so their
//! serialized bytes never change unless the format does.

use std::collections::BTreeMap;

use journal_model::{
    Attachment, Entry, EntryId, Manifest, ManifestEntry, ObjectName, Tombstone,
    ENTRY_FORMAT_VERSION, MANIFEST_FORMAT_VERSION, OBJECT_NAME_LEN,
};
use uuid::Uuid;

const ENTRY_GOLDEN: &str = include_str!("golden/entry.json");
const MANIFEST_GOLDEN: &str = include_str!("golden/manifest.json");

fn fixed_entry_id() -> Result<EntryId, Box<dyn std::error::Error>> {
    Ok(EntryId::from_uuid(Uuid::parse_str("11111111-1111-4111-8111-111111111111")?))
}

fn fixed_tombstoned_id() -> Result<EntryId, Box<dyn std::error::Error>> {
    Ok(EntryId::from_uuid(Uuid::parse_str("22222222-2222-4222-8222-222222222222")?))
}

fn fixed_object_name() -> ObjectName {
    ObjectName::from_bytes([0xab; OBJECT_NAME_LEN])
}

fn sample_entry() -> Result<Entry, Box<dyn std::error::Error>> {
    Ok(Entry {
        format_version: ENTRY_FORMAT_VERSION,
        id: fixed_entry_id()?,
        title: "First light".to_string(),
        body: "Woke early, walked the long way.\n\n![sunrise](journal://blob/abc)".to_string(),
        tags: vec!["mornings".to_string(), "walks".to_string()],
        created_at: 1_755_216_000,
        updated_at: 1_755_216_400,
        attachments: vec![Attachment {
            name: fixed_object_name(),
            content_type: "image/jpeg".to_string(),
        }],
    })
}

fn sample_manifest() -> Result<Manifest, Box<dyn std::error::Error>> {
    let mut entries = BTreeMap::new();
    entries.insert(fixed_entry_id()?, ManifestEntry { object_name: fixed_object_name() });
    let mut tombstones = BTreeMap::new();
    tombstones.insert(fixed_tombstoned_id()?, Tombstone { deleted_at: 1_755_300_000 });
    Ok(Manifest { format_version: MANIFEST_FORMAT_VERSION, entries, tombstones })
}

#[test]
fn entry_bytes_match_golden_file() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = sample_entry()?.to_bytes()?;
    assert_eq!(
        String::from_utf8(bytes)?,
        ENTRY_GOLDEN.trim_end(),
        "Entry's serialized shape changed — bump ENTRY_FORMAT_VERSION and add a migration \
         or a hard refusal, then update the golden fixture deliberately."
    );
    Ok(())
}

#[test]
fn manifest_bytes_match_golden_file() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = sample_manifest()?.to_bytes()?;
    assert_eq!(
        String::from_utf8(bytes)?,
        MANIFEST_GOLDEN.trim_end(),
        "Manifest's serialized shape changed — bump MANIFEST_FORMAT_VERSION and add a \
         migration or a hard refusal, then update the golden fixture deliberately."
    );
    Ok(())
}

#[test]
fn golden_entry_file_parses_back_to_the_sample() -> Result<(), Box<dyn std::error::Error>> {
    let parsed = Entry::from_bytes(ENTRY_GOLDEN.trim_end().as_bytes())?;
    assert_eq!(parsed, sample_entry()?);
    Ok(())
}

#[test]
fn golden_manifest_file_parses_back_to_the_sample() -> Result<(), Box<dyn std::error::Error>> {
    let parsed = Manifest::from_bytes(MANIFEST_GOLDEN.trim_end().as_bytes())?;
    assert_eq!(parsed, sample_manifest()?);
    Ok(())
}
