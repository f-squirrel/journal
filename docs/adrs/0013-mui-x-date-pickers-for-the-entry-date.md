# 0013. Use `@mui/x-date-pickers` for the entry date control

## Status

Accepted — 2026-09-20

Amended 2026-09-21: `DateCalendar` has a second entry point. The Calendar
view (`pages/CalendarPage.tsx`) renders it on the page, not in a `Popover`,
with a custom `slots.day` that marks days with entries. The Decision's "no
other entry point" no longer holds; `reduceAnimations` applies there for the
same reason, and the adapter and dependencies are unchanged.

## Context

`Header` grew an `entry_date`: the day an entry is *about*, which the list
now sorts and groups by instead of `updated_at`. An entry written up the
next morning is the normal case for a journal, so the date has to be
correctable — which means the app needs a date control, and it had none.

MUI v6 core (ADR 0002) does not ship one: pickers live in the separate
`@mui/x-date-pickers` package, which in turn requires a date-library
adapter.

## Decision

Add `@mui/x-date-pickers` (v8) with `date-fns` (v4) as its adapter. The
adapter is installed once as a `LocalizationProvider` inside the
`ThemeProvider` in `App.tsx`. The only picker used is `DateCalendar`, in a
`Popover` anchored on the entry pane's date heading — clicking the heading
is how the date is changed, and there is no other entry point.

`reduceAnimations` is set on the calendar: the month slide transition is
noise in a popover that small, and it keeps the outgoing month mounted,
where it shadows the month actually on screen.

## Reasons

- **v8 against MUI v6 is supported, and v6 of the pickers is not.** The
  picker's peer ranges were checked rather than assumed: v8 accepts
  `@mui/material` `^5.15.14 || ^6 || ^7` and React `^17 || ^18 || ^19`,
  which this project satisfies. `@mui/x-date-pickers` v6 caps React at 18
  and so cannot be used here at all. The install produced no peer warnings
  and `tsc --noEmit` stayed clean.
- **`date-fns` over dayjs/luxon/moment** because it is plain `Date` in and
  plain `Date` out. The app's timestamps are ISO strings on the wire and on
  disk (`DateTime<Utc>` in Rust), so no library-specific date type ever
  needs to enter the codebase — the picker hands back a `Date`, the pane
  writes an ISO string, and nothing in between has to know what the adapter
  is.
- Using MUI's own picker keeps the control consistent with the rest of the
  UI in both light and dark mode for free, which matters because the
  heading it replaces is the largest thing on the pane.

## Alternatives considered

- **A native `<input type="date">` in a `Popover`**: zero new dependencies,
  and genuinely tempting for an app this small. Rejected because the
  calendar it opens is the OS's, so it is unthemed, inconsistent between
  the three platforms this app targets, and inconsistent with the MUI
  dialogs beside it. The dependency buys a control that looks like the rest
  of the app on desktop, iOS and Android alike.
- **Hand-rolling a month grid**: a calendar is more fiddly than it looks
  (week starts, locale, keyboard navigation, focus management), and ADR
  0002 already chose not to hand-build standard components.
- **No date editing at all — set `entry_date` once at creation**: rejected
  because a date that cannot be corrected is worse than no date field. It
  would have made re-dating a mis-filed entry impossible without editing
  the store by hand.

## Consequences

- Two new runtime dependencies, and a `LocalizationProvider` that every
  future picker in the app will share.
- The production bundle grew; it was already over Vite's 500 kB warning
  threshold, and code-splitting remains unaddressed either way.
- `@mui/x-date-pickers` v8 tracks MUI v7's styling engine. This is fine
  today, but if ADR 0002's MUI v6 pin is ever revisited, the picker and the
  core should be moved together.
