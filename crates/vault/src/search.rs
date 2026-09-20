//! Finding a query in an entry's text, and the snippet that shows where.
//!
//! Pure string work, kept out of [`Vault`](crate::Vault) because none of it
//! needs a store or a key: the vault decrypts, this decides what matched.
//!
//! Matching is case-insensitive substring, and deliberately nothing more —
//! no regex, no stemming, no ranking. See the plan's F2 section.

/// Characters of context around the first match, in total. Wide enough to read
/// a phrase, short enough for a list row.
const SNIPPET_CHARS: usize = 80;

/// How much of that window sits before the match, so the match itself is
/// visible rather than pushed off the right edge.
const SNIPPET_LEAD: usize = 24;

/// A query, folded once so it can be run against many entries.
///
/// Holding it as a type rather than a `&str` keeps the "already lowercased"
/// precondition from being something each caller has to remember.
pub(crate) struct Query(String);

impl Query {
    /// The query as the user typed it, or `None` if there is nothing to search
    /// for. An empty query matches nothing, rather than everything: the list
    /// already shows everything.
    pub(crate) fn new(query: &str) -> Option<Self> {
        let trimmed = query.trim();
        (!trimmed.is_empty()).then(|| Self(lowercase(trimmed).0))
    }

    /// Where `haystack` first contains this query, as a byte offset into
    /// `haystack` itself.
    fn find(&self, haystack: &str) -> Option<usize> {
        let (folded, offsets) = lowercase(haystack);
        folded.find(&self.0).map(|at| offsets[at])
    }

    pub(crate) fn matches(&self, haystack: &str) -> bool {
        self.find(haystack).is_some()
    }

    /// The text to show for a match in `markdown`: a window around the first
    /// occurrence, or the opening of the entry when the match was in the title
    /// and not the body.
    pub(crate) fn snippet(&self, markdown: &str) -> String {
        let at = self.find(markdown).unwrap_or(0);
        window(markdown, at)
    }
}

/// A lowercased copy of `s`, plus, for each byte of it, the byte offset in `s`
/// of the character that produced it.
///
/// The offsets are the reason this is not `s.to_lowercase()`: folding can
/// change a string's length (`İ` lowercases to two chars), so an index into
/// the folded text is not an index into the original, and slicing the original
/// with one would cut a character in half — a panic, on a query the user can
/// type. The trailing entry maps the end of the folded text to the end of `s`.
fn lowercase(s: &str) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(s.len());
    let mut offsets = Vec::with_capacity(s.len() + 1);
    for (at, c) in s.char_indices() {
        for lower in c.to_lowercase() {
            folded.push(lower);
            offsets.resize(folded.len(), at);
        }
    }
    offsets.push(s.len());
    (folded, offsets)
}

/// [`SNIPPET_CHARS`] characters of `text` around the byte offset `at`, with
/// runs of whitespace collapsed to single spaces and an ellipsis on each side
/// that was cut.
///
/// Collapsing happens after the window is taken, not before: the offset is
/// into `text` as it is, and rewriting the text first would move it.
fn window(text: &str, at: usize) -> String {
    let mut start = text[..at]
        .char_indices()
        .rev()
        .nth(SNIPPET_LEAD - 1)
        .map_or(0, |(i, _)| i);

    // A window cut mid-word opens on half of one ("…o write things down"), so
    // begin at the first word break instead. Unless there is none before the
    // match — then the match is *inside* that half-word, and showing it cut is
    // better than not showing it.
    if start > 0 {
        if let Some(space) = text[start..at].find(char::is_whitespace) {
            start += space;
        }
    }

    let mut end = text.len();
    for (taken, (i, _)) in text[start..].char_indices().enumerate() {
        if taken == SNIPPET_CHARS {
            end = start + i;
            break;
        }
    }

    let mut snippet = String::with_capacity(end - start + 2);
    if start > 0 {
        snippet.push('…');
    }
    snippet.push_str(&collapse(&text[start..end]));
    if end < text.len() {
        snippet.push('…');
    }
    snippet
}

/// Every run of whitespace as a single space, trimmed. A markdown body is full
/// of newlines and indentation; a row is one line tall.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(s: &str) -> Query {
        Query::new(s).expect("a non-empty query")
    }

    #[test]
    fn an_empty_query_is_not_a_query() {
        assert!(Query::new("").is_none());
        assert!(Query::new("   \n ").is_none());
    }

    #[test]
    fn matching_ignores_case_on_both_sides() {
        assert!(query("COFFEE").matches("a cup of Coffee"));
        assert!(query("coffee").matches("COFFEE"));
        assert!(!query("tea").matches("a cup of coffee"));
    }

    /// The trap the offset table exists for: `İ` is one character that folds to
    /// two, so an index into the folded text is past the character it names in
    /// the original. Slicing with it used to split a character and panic.
    #[test]
    fn a_match_after_a_character_that_grows_when_folded_does_not_panic() {
        let text = "İstanbul, and then some coffee in the evening";
        let snippet = query("coffee").snippet(text);
        assert!(snippet.contains("coffee"), "got {snippet:?}");
    }

    #[test]
    fn the_snippet_is_a_window_around_the_first_match() {
        let text = format!("{}NEEDLE{}", "a".repeat(200), "b".repeat(200));
        let snippet = query("needle").snippet(&text);

        assert!(snippet.contains("NEEDLE"), "got {snippet:?}");
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
        assert!(snippet.chars().count() <= SNIPPET_CHARS + 2);
    }

    #[test]
    fn the_snippet_does_not_open_on_half_a_word() {
        let text = "Started journaling again. Feels good to write things down.";
        let snippet = query("write").snippet(text);

        // Not "…ing again.", which is where 24 characters back actually lands.
        assert_eq!(snippet, "…again. Feels good to write things down.");
    }

    /// The exception to the rule above: with no word break between the start
    /// of the window and the match, cutting to one would cut the match away.
    #[test]
    fn a_match_inside_the_half_word_is_shown_cut_rather_than_dropped() {
        let text = format!("{}needle", "x".repeat(200));
        let snippet = query("needle").snippet(&text);

        assert!(snippet.contains("needle"), "got {snippet:?}");
    }

    #[test]
    fn a_short_entry_is_its_own_snippet_with_no_ellipsis() {
        assert_eq!(query("walk").snippet("A long walk."), "A long walk.");
    }

    #[test]
    fn the_snippet_collapses_the_markdown_onto_one_line() {
        let text = "# Title\n\n- one\n- two\n\n  indented";
        assert_eq!(query("two").snippet(text), "# Title - one - two indented");
    }

    /// A title-only match still gets a snippet: the entry's opening, which is
    /// the most useful thing to show when the body has nothing to point at.
    #[test]
    fn a_match_the_body_does_not_contain_shows_the_opening() {
        let text = "Nothing about the query here at all.";
        assert_eq!(query("elsewhere").snippet(text), text);
    }

    #[test]
    fn an_empty_body_has_an_empty_snippet() {
        assert_eq!(query("anything").snippet(""), "");
    }
}
