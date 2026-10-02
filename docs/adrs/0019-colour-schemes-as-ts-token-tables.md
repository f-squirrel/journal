# 0019. Define colour schemes as TS token tables chosen by a device setting

## Status

Accepted — 2026-10-02

## Decision

Every colour and the entry font live in `src/schemes.ts`, one token table per
scheme with a light and a dark variant. The backend stores only the scheme's
id, as `DeviceSettings.color_scheme` (a `ColorScheme` enum, device-only,
default `Parchment`). `getTheme(mode, scheme)` builds the MUI theme from the
table, so the scheme is orthogonal to the colour mode.

## Reasons

Only the frontend renders colours, so shipping them through `invoke` would
only bring them back to where they are used. Editing a hex hot-reloads under
`npm run dev`, and `schemes.ts` has no app imports, so the contrast spec
loads it directly in Node. It is the same split as shortcuts (ADR 0015) and
`color_mode` (ADR 0016): the backend keeps the choice, the frontend owns what
it means. An unknown stored id decodes to the default.

## Alternatives considered

- **MUI `cssVariables` with `colorSchemes`, schemes as CSS blocks.** Switches
  without rebuilding the theme, but `theme.palette.tonal` is read through `sx`
  in many components and CodeMirror needs JS values. A memoised `createTheme`
  per mode and scheme is cheap at four schemes. Revisit if schemes become
  numerous or user-defined.
- **A synced scheme (`JournalSettings`).** The look is a per-device
  preference.
- **User-editable palettes.** Needs contrast validation and import/export.
- **Bundled web fonts.** A dependency and bundle size; system stacks give the
  serif look for free.
