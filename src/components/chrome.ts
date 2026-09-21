/// The shell's dimensions and the type scale, in one place because they have
/// to agree — and all of them in `rem`, never `px`.
///
/// `rem` is relative to the root font size, which is the browser's (and, in a
/// webview, the platform's) text-size setting. A journal is text, so when that
/// setting goes up the columns, headers and gutters have to grow with the
/// words inside them: a rail sized in `px` clips its icons, and a 56px header
/// clips a title that just got bigger. Everything here scales together, and at
/// the default 16px root it is pixel-for-pixel what it was.
///
/// What is *not* here, on purpose: nothing is sized against the viewport
/// (`vw`/`vh`). The list column is a column, not a percentage — on a 34"
/// monitor a proportional one would be absurd, and on a phone the layout drops
/// to one column at `md` instead (see `JournalLayout`). Text sized in `vw`
/// ignores the reader's own font size, which is the setting that matters here.

/// `px` at the default root size, as `rem`. The design is drawn in pixels, so
/// the call sites keep saying what was drawn and this says what it means.
export const rem = (px: number) => `${px / 16}rem`;

/// The icon rail is one 40px button plus its gutters. Every column's header is
/// `HEADER_HEIGHT` tall with a bottom border, so the hairline runs unbroken
/// across the sidebar, the list and the entry pane — change it here or the
/// three fall out of line.
export const SIDEBAR_WIDTH = rem(56);
export const LIST_WIDTH = rem(320);
export const HEADER_HEIGHT = rem(56);

/// The entry pane's measure: how wide a line of the journal is allowed to get
/// before it is tiring to read. It caps the column against the window, and
/// being in `rem` it is a count of characters rather than of pixels, which is
/// what a measure actually is.
export const READING_WIDTH = rem(720);

/// The entry body's floor, in both modes. Read mode and edit mode must claim
/// the same height or switching between them makes the pane jump.
export const BODY_MIN_HEIGHT = rem(260);

/// The type scale. Sizes are the drawn pixel values; tracking is in `em`, so
/// it stays proportional to whatever size it lands on.
export const TYPE = {
  /// The weekday under a row's day number.
  micro: rem(10),
  /// Uppercase section headings — the list's months, the calendar's years.
  label: rem(11),
  /// A row's time, and a search result's line of text.
  meta: rem(12),
  /// The search field and the list column's alerts.
  dense: rem(13),
  /// A row's title.
  row: rem(14),
  /// A column header's title.
  header: rem(17),
  /// A row's day number.
  lead: rem(20),
  /// An entry's own title, in both modes.
  entryTitle: "1.45rem",
} as const;

/// Tracking for the uppercase section headings, which need it opened up, and
/// for the display sizes, which need it closed.
export const TRACKING = { wide: "0.07em", tight: "-0.013em" } as const;

/// The shared style of a section heading: the entry list's month rules and the
/// calendar's year rules are the same heading in two places, and look it.
export const sectionHeadingSx = {
  color: "text.secondary",
  fontSize: TYPE.label,
  fontWeight: 700,
  letterSpacing: TRACKING.wide,
  textTransform: "uppercase",
  lineHeight: 2.6,
} as const;
