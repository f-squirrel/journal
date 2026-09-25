//! Plaintext export: every entry as a markdown file in a folder the user chose.
//!
//! The one place the app deliberately writes unencrypted journal content to
//! disk — it is the backup, and the answer to "am I locked into this app?".
//! Nothing here is logged beyond a count (the command does that), and nothing
//! here touches the vault's own store: it reads through [`Vault`] like any
//! command and writes somewhere the vault and sync never look.
//!
//! Glue over the vault with no reusable core, so a module and not a crate
//! (`CLAUDE.md`: `src-tauri` stays thin, *backend crates* go under `crates/`).
//! Synchronous like every layer below the command boundary.

use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate, SecondsFormat, Utc};
use journal_api::{Entry, Header};
use journal_vault::{Vault, VaultError};
use thiserror::Error;

/// The longest title slug, in bytes. A filename is capped at 255 bytes on the
/// filesystems the app targets, and the date, an id suffix and the extension
/// share that budget with a slug that may be all four-byte characters.
const MAX_SLUG_BYTES: usize = 80;

/// What a title with no letters or digits in it is filed under.
const UNTITLED: &str = "untitled";

/// Rendered for the user, so each says what to do rather than what happened.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error("that folder does not exist")]
    NotADirectory,
    #[error("that folder is not empty; choose an empty one so nothing is overwritten")]
    NotEmpty,
    #[error("that folder is inside the journal's own data; choose another one")]
    InsideAppData,
    #[error("could not write the export: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Vault(#[from] VaultError),
}

/// Writes every entry in `vault` to `directory`, one `.md` file each, and
/// returns how many.
///
/// `directory` must exist and be empty, and must not be inside `app_data` (the
/// app's own data directory, where the vault and its key live): an export never
/// overwrites anything and never lands among the encrypted blobs.
///
/// **Not atomic, and does not try to be.** `FsStore::put` writes a temp file and
/// renames it because a torn blob is an entry lost; an export is a copy, the
/// original is untouched, and the folder is empty when it starts. What it does
/// guarantee is that a failure does not strand the user: the files this call
/// created are removed again, so the folder is empty and the same export can be
/// retried — otherwise the empty-folder rule would turn one failed run into a
/// permanent refusal. (A crash mid-export leaves a partial folder; the journal
/// is intact and the next export goes to a fresh one.)
pub fn to_directory(
    vault: &Vault,
    directory: &Path,
    app_data: &Path,
) -> Result<usize, ExportError> {
    let directory = fs::canonicalize(directory).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => ExportError::NotADirectory,
        _ => ExportError::Io(e),
    })?;
    if !directory.is_dir() {
        return Err(ExportError::NotADirectory);
    }
    // If the data directory cannot be resolved it does not exist, and nothing
    // can be inside a directory that is not there.
    let app_data = fs::canonicalize(app_data).unwrap_or_else(|_| app_data.to_path_buf());
    if directory.starts_with(&app_data) {
        return Err(ExportError::InsideAppData);
    }
    if fs::read_dir(&directory)?.next().is_some() {
        return Err(ExportError::NotEmpty);
    }

    let mut headers = vault.list_entries()?;
    // The listing's order among entries of the same moment is whatever the
    // directory gave back. Pinning it makes the names — which one of two
    // same-titled entries keeps the plain name — the same on every export.
    headers.sort_by_key(|h| (Reverse(h.entry_date), h.id.to_string()));

    write_entries(vault, &directory, &headers)
}

/// Writes the entries `headers` names, removing what it wrote if any of it
/// fails. Split out so the cleanup can be tested without the checks above.
fn write_entries(
    vault: &Vault,
    directory: &Path,
    headers: &[Header],
) -> Result<usize, ExportError> {
    let mut written: Vec<PathBuf> = Vec::new();
    let result = (|| {
        let mut taken = HashSet::new();
        for header in headers {
            // Read again rather than rendered from the listing's header, which
            // has no body — and which a save or a sync pass may already have
            // outdated. Gone now means deleted since the listing: not exported.
            let Some(entry) = vault.get_entry(&header.id)? else {
                continue;
            };
            let path = directory.join(file_name(&entry.header, &mut taken));
            // `create_new` so that overwriting is impossible even if a name
            // somehow repeats, rather than merely unlikely. Recorded before the
            // write so a write that fails half-way is cleaned up too.
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            // Plaintext journal: readable by its owner and nobody else, whatever
            // the umask and whatever the chosen folder allows. The dialog
            // promises "anyone that can read this folder"; 0644 is more people
            // than that. Windows has no equivalent here.
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&path)?;
            written.push(path);
            file.write_all(render(&entry).as_bytes())?;
        }
        Ok(written.len())
    })();
    if result.is_err() {
        for path in &written {
            fs::remove_file(path).ok();
        }
    }
    result
}

/// The name for an entry: `YYYY-MM-DD-slug.md`, with the id spliced in when
/// that name is already `taken`.
fn file_name(header: &Header, taken: &mut HashSet<String>) -> String {
    let day = local_day(header.entry_date);
    let slug = slugify(&header.title);
    let mut name = format!("{day}-{slug}.md");
    if taken.contains(&name) {
        name = format!("{day}-{slug}-{}.md", header.id);
    }
    taken.insert(name.clone());
    name
}

/// The calendar day an entry date falls on *for the person reading it*: the
/// app draws every date in the local zone, so a file named for the UTC day
/// would disagree with the heading of the entry it holds for anyone writing
/// late in the evening.
fn local_day(entry_date: DateTime<Utc>) -> NaiveDate {
    entry_date.with_timezone(&Local).date_naive()
}

/// A title as a filename fragment: lower-case letters and digits (of any
/// script — a journal is not necessarily in English), everything else
/// collapsed to single hyphens, none at either end.
///
/// Never empty: a title with nothing to keep is filed as `untitled`. It cannot
/// contain a path separator or a character a filesystem refuses, since neither
/// is alphanumeric.
fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut gap = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            if gap && !slug.is_empty() {
                slug.push('-');
            }
            gap = false;
            slug.extend(c.to_lowercase());
        } else {
            gap = true;
        }
    }

    let mut end = slug.len().min(MAX_SLUG_BYTES);
    while !slug.is_char_boundary(end) {
        end -= 1;
    }
    slug.truncate(end);
    // A cut can leave the hyphen that separated two words.
    slug.truncate(slug.trim_end_matches('-').len());

    if slug.is_empty() {
        UNTITLED.to_string()
    } else {
        slug
    }
}

/// The YAML block at the top of each file: what the filename cannot carry, so
/// that an import could round-trip an entry exactly.
///
/// The title is a JSON string, which is also a valid YAML double-quoted
/// scalar, so quotes, colons and newlines in a title need no escaping logic
/// of their own. Timestamps are RFC 3339 in UTC at full precision.
fn front_matter(header: &Header) -> String {
    let title = serde_json::to_string(&header.title).expect("a string always serializes");
    format!(
        "---\nid: {}\ntitle: {title}\nentry_date: {}\nupdated_at: {}\n---\n",
        header.id,
        stamp(header.entry_date),
        stamp(header.updated_at),
    )
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// The whole file: front matter, a blank line, the markdown as written, and a
/// closing newline if the markdown did not end with one.
fn render(entry: &Entry) -> String {
    let mut text = front_matter(&entry.header);
    text.push('\n');
    text.push_str(&entry.markdown);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use journal_api::Id;
    use journal_fs_store::FsStore;
    use journal_store::BlobStore;
    use secrecy::SecretString;

    /// A directory nothing else in this process will pick, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "journal-export-test-{}-{n}-{label}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        rfc3339.parse().expect("a valid timestamp")
    }

    fn entry(title: &str, entry_date: &str, markdown: &str) -> Entry {
        Entry {
            header: Header {
                id: Id::new(),
                title: title.to_string(),
                updated_at: at("2026-09-20T10:00:00Z"),
                entry_date: at(entry_date),
            },
            markdown: markdown.to_string(),
        }
    }

    /// A vault holding `entries`, under `home` — a stand-in for the app's data
    /// directory, so what the tests export *from* is somewhere they can also
    /// point the "inside the app's data" check at.
    fn vault_with(home: &Scratch, entries: &[&Entry]) -> Vault {
        let store: Arc<dyn BlobStore> = Arc::new(FsStore::new(home.0.join("entries")));
        let vault = Vault::open(store, &SecretString::from("passphrase".to_string()))
            .expect("a fresh store opens as a new vault");
        for entry in entries {
            vault.save_entry(entry).expect("the entry saves");
        }
        vault
    }

    /// The names in `directory`, sorted.
    fn names(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn day(entry: &Entry) -> String {
        local_day(entry.header.entry_date).to_string()
    }

    #[test]
    fn slugify_lowercases_and_joins_words_with_hyphens() {
        assert_eq!(slugify("First Day Back"), "first-day-back");
    }

    #[test]
    fn slugify_collapses_punctuation_and_trims_the_ends() {
        assert_eq!(slugify("  ...Hello,   world!?  "), "hello-world");
        assert_eq!(slugify("a/b\\c:d*e"), "a-b-c-d-e");
    }

    #[test]
    fn slugify_keeps_letters_of_any_script() {
        assert_eq!(slugify("Café au lait"), "café-au-lait");
        assert_eq!(slugify("Первый день"), "первый-день");
        assert_eq!(slugify("日記 2026"), "日記-2026");
    }

    #[test]
    fn slugify_files_a_title_with_nothing_to_keep_as_untitled() {
        assert_eq!(slugify(""), UNTITLED);
        assert_eq!(slugify("   "), UNTITLED);
        assert_eq!(slugify("?!…—"), UNTITLED);
    }

    #[test]
    fn slugify_cuts_long_titles_on_a_character_boundary_without_a_dangling_hyphen() {
        let long = "word ".repeat(40);
        let slug = slugify(&long);
        assert!(slug.len() <= MAX_SLUG_BYTES);
        assert!(!slug.ends_with('-'), "got {slug:?}");

        // Four-byte characters, so a cut at the byte limit would split one.
        let wide = "😀a".repeat(60) + &"ä".repeat(60);
        let slug = slugify(&wide);
        assert!(slug.len() <= MAX_SLUG_BYTES);
        assert!(slug.is_char_boundary(slug.len()));
        assert!(!slug.is_empty());
    }

    #[test]
    fn front_matter_carries_the_four_fields_a_round_trip_needs() {
        let e = entry("First day back", "2026-06-12T07:30:00Z", "");
        assert_eq!(
            front_matter(&e.header),
            format!(
                "---\nid: {}\ntitle: \"First day back\"\nentry_date: 2026-06-12T07:30:00Z\nupdated_at: 2026-09-20T10:00:00Z\n---\n",
                e.header.id
            )
        );
    }

    #[test]
    fn front_matter_escapes_a_title_that_would_break_yaml() {
        let e = entry(
            "He said: \"no\"\n# not a heading",
            "2026-06-12T07:30:00Z",
            "",
        );
        let block = front_matter(&e.header);
        // Still one line for the title, and the quotes and newline escaped.
        let title = block.lines().nth(2).unwrap();
        assert_eq!(title, r#"title: "He said: \"no\"\n# not a heading""#);
    }

    #[test]
    fn front_matter_keeps_sub_second_precision() {
        let mut e = entry("t", "2026-06-12T07:30:00Z", "");
        e.header.updated_at = at("2026-09-20T10:00:00.123456Z");
        assert!(front_matter(&e.header).contains("updated_at: 2026-09-20T10:00:00.123456Z\n"));
    }

    #[test]
    fn render_puts_the_markdown_after_the_front_matter_and_ends_with_a_newline() {
        let e = entry("t", "2026-06-12T07:30:00Z", "# Heading\n\nBody");
        let text = render(&e);
        assert!(text.starts_with("---\nid: "));
        assert!(text.ends_with("---\n\n# Heading\n\nBody\n"), "got {text:?}");

        // Already newline-terminated: not doubled.
        let e = entry("t", "2026-06-12T07:30:00Z", "Body\n");
        assert!(render(&e).ends_with("---\n\nBody\n"));
    }

    #[test]
    fn exports_one_named_file_per_entry_with_its_text() {
        let home = Scratch::new("home");
        let out = Scratch::new("out");
        let a = entry(
            "First day back",
            "2026-06-12T07:30:00Z",
            "# First\n\nHello.",
        );
        let b = entry("Project ideas", "2026-09-02T12:00:00Z", "Some ideas.");
        let c = entry("Café!", "2026-09-03T12:00:00Z", "Coffee.");
        let vault = vault_with(&home, &[&a, &b, &c]);

        let count = to_directory(&vault, &out.0, &home.0).expect("the export succeeds");

        assert_eq!(count, 3);
        let mut expected = vec![
            format!("{}-first-day-back.md", day(&a)),
            format!("{}-project-ideas.md", day(&b)),
            format!("{}-café.md", day(&c)),
        ];
        expected.sort();
        assert_eq!(names(&out.0), expected);

        let text = fs::read_to_string(out.0.join(expected_name(&a))).unwrap();
        assert_eq!(text, render(&a));
    }

    fn expected_name(e: &Entry) -> String {
        format!("{}-{}.md", day(e), slugify(&e.header.title))
    }

    #[test]
    fn two_entries_with_one_title_and_day_get_distinct_names_the_same_every_time() {
        let home = Scratch::new("home");
        let one = entry("Notes", "2026-09-02T12:00:00Z", "one");
        let two = entry("Notes", "2026-09-02T12:00:00Z", "two");
        let vault = vault_with(&home, &[&one, &two]);

        let mut runs = Vec::new();
        for label in ["a", "b"] {
            let out = Scratch::new(label);
            assert_eq!(to_directory(&vault, &out.0, &home.0).unwrap(), 2);
            runs.push(names(&out.0));
        }

        let names = &runs[0];
        assert_eq!(names.len(), 2, "one overwrote the other: {names:?}");
        assert!(names.contains(&expected_name(&one)), "got {names:?}");
        assert_eq!(runs[0], runs[1], "the names depend on the run");
        // The one that lost the plain name carries its id.
        let suffixed = names.iter().find(|n| **n != expected_name(&one)).unwrap();
        let ids = [one.header.id.to_string(), two.header.id.to_string()];
        assert!(
            ids.iter().any(|id| suffixed.contains(id)),
            "got {suffixed:?}"
        );
    }

    #[test]
    fn refuses_a_folder_that_is_not_empty_and_writes_nothing() {
        let home = Scratch::new("home");
        let out = Scratch::new("out");
        fs::write(out.0.join("mine.txt"), "keep me").unwrap();
        let vault = vault_with(&home, &[&entry("t", "2026-09-02T12:00:00Z", "b")]);

        let err = to_directory(&vault, &out.0, &home.0).unwrap_err();

        assert!(matches!(err, ExportError::NotEmpty), "got {err}");
        assert_eq!(names(&out.0), ["mine.txt"]);
        assert_eq!(
            fs::read_to_string(out.0.join("mine.txt")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn refuses_the_app_data_directory_and_anything_inside_it() {
        let home = Scratch::new("home");
        let vault = vault_with(&home, &[&entry("t", "2026-09-02T12:00:00Z", "b")]);
        let inside = home.0.join("exports");
        fs::create_dir(&inside).unwrap();

        for target in [&home.0, &inside] {
            let err = to_directory(&vault, target, &home.0).unwrap_err();
            assert!(matches!(err, ExportError::InsideAppData), "got {err}");
        }
        assert!(names(&inside).is_empty());
    }

    #[test]
    fn refuses_a_folder_that_does_not_exist_or_is_a_file() {
        let home = Scratch::new("home");
        let vault = vault_with(&home, &[]);
        let file = home.0.join("a-file");
        fs::write(&file, "x").unwrap();
        let elsewhere = Scratch::new("elsewhere");

        for target in [elsewhere.0.join("missing"), file] {
            let err = to_directory(&vault, &target, &elsewhere.0.join("data")).unwrap_err();
            assert!(matches!(err, ExportError::NotADirectory), "got {err}");
        }
    }

    #[test]
    fn exports_an_empty_journal_as_zero_files() {
        let home = Scratch::new("home");
        let out = Scratch::new("out");
        let vault = vault_with(&home, &[]);

        assert_eq!(to_directory(&vault, &out.0, &home.0).unwrap(), 0);
        assert!(names(&out.0).is_empty());
    }

    /// The files are the whole journal in plaintext, so they are the owner's
    /// alone whatever the umask: a default `create` would give 0644 under the
    /// usual 022.
    #[cfg(unix)]
    #[test]
    fn exported_files_are_readable_by_their_owner_alone() {
        use std::os::unix::fs::PermissionsExt;

        let home = Scratch::new("home");
        let out = Scratch::new("out");
        let a = entry("First", "2026-09-02T12:00:00Z", "one");
        let b = entry("Second", "2026-09-03T12:00:00Z", "two");
        let vault = vault_with(&home, &[&a, &b]);

        assert_eq!(to_directory(&vault, &out.0, &home.0).unwrap(), 2);

        for name in names(&out.0) {
            let mode = fs::metadata(out.0.join(&name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name} is mode {:o}", mode & 0o777);
        }
    }

    /// The reason for the cleanup: without it a failed export leaves a folder
    /// the empty-folder rule then refuses, and one bad run is a permanent one.
    #[test]
    fn a_failed_export_removes_what_it_wrote() {
        let home = Scratch::new("home");
        let out = Scratch::new("out");
        let newer = entry("Newer", "2026-09-03T12:00:00Z", "n");
        let older = entry("Older", "2026-09-02T12:00:00Z", "o");
        let vault = vault_with(&home, &[&newer, &older]);
        let headers = vec![newer.header.clone(), older.header.clone()];
        // Something already at the second entry's name, which the export
        // writes after the first: `create_new` refuses, and the first goes.
        // (Called below the emptiness check, which would refuse this folder.)
        fs::write(out.0.join(expected_name(&older)), "not yours").unwrap();

        let err = write_entries(&vault, &out.0, &headers).unwrap_err();

        assert!(matches!(err, ExportError::Io(_)), "got {err}");
        assert_eq!(names(&out.0), [expected_name(&older)]);
        assert_eq!(
            fs::read_to_string(out.0.join(expected_name(&older))).unwrap(),
            "not yours"
        );
    }
}
