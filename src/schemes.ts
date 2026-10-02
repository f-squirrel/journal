// Colour schemes: every colour the app uses, in one place.
//
// To try a colour: edit a hex below, save, and `npm run dev` hot-reloads the
// page. A scheme is a handful of base colours per mode; `derive` fills in the
// rest (surface shades, outlines, containers, the app bar). Any derived value
// can be overridden by hand, and `parchment` spells everything out so its
// look cannot drift. To add a scheme: add an id to `ColorScheme` in
// `crates/settings/src/device.rs`, run `make bindings`, then add an entry
// here and to `SCHEME_ORDER`.
//
// This file must stay free of imports from the app and from MUI: the contrast
// spec (`e2e/color-scheme.spec.ts`) loads it directly in Node.

import type { ColorScheme } from "./types";

export interface Tonal {
  primaryContainer: string;
  onPrimaryContainer: string;
  secondaryContainer: string;
  onSecondaryContainer: string;
  surfaceContainerLowest: string;
  surfaceContainerLow: string;
  surfaceContainer: string;
  surfaceContainerHigh: string;
  surfaceContainerHighest: string;
  outline: string;
  outlineVariant: string;
}

interface Swatch {
  main: string;
  light: string;
  dark: string;
  contrastText: string;
}

/// Every token the theme needs, fully spelled out.
export interface SchemeColors {
  primary: Swatch;
  secondary: Swatch;
  background: { default: string; paper: string };
  text: { primary: string; secondary: string };
  tonal: Tonal;
  appBar: { background: string };
}

export interface SchemeDef {
  label: string;
  /// Font stack for entry text (editor, rendered markdown, entry title).
  /// System stacks only: nothing to bundle.
  entryFont: string;
  light: SchemeColors;
  dark: SchemeColors;
}

const SANS = '"Roboto", "Helvetica Neue", Arial, sans-serif';
const SERIF =
  'Charter, "Iowan Old Style", "Palatino Linotype", Palatino, Georgia, serif';

// ---------------------------------------------------------------------------
// The short form: what you edit to try a new look.

interface Base {
  /// The page behind everything.
  background: string;
  /// Cards, dialogs, the entry pane.
  paper: string;
  /// Body text.
  text: string;
  primary: string;
  secondary: string;
  /// The darkest (light mode) or lightest (dark mode) surface shade; the
  /// shades between it and `background` are interpolated.
  surface: string;
  /// How opaque `text` is for secondary text, 0..1. Default 0.64.
  mutedText?: number;
}

type Rgb = [number, number, number];

function parse(hex: string): Rgb {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function toHex([r, g, b]: Rgb): string {
  const part = (v: number) =>
    Math.round(Math.min(255, Math.max(0, v)))
      .toString(16)
      .padStart(2, "0");
  return `#${part(r)}${part(g)}${part(b)}`.toUpperCase();
}

/// `t` of the way from `a` to `b`.
function mix(a: string, b: string, t: number): string {
  const [ar, ag, ab] = parse(a);
  const [br, bg, bb] = parse(b);
  return toHex([ar + (br - ar) * t, ag + (bg - ag) * t, ab + (bb - ab) * t]);
}

function rgba(hex: string, alpha: number): string {
  const [r, g, b] = parse(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

function luminance(hex: string): number {
  const lin = (v: number) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const [r, g, b] = parse(hex);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/// WCAG 2.x contrast ratio between two opaque hex colours.
export function contrastRatio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/// Whichever of near-black and white reads better on `bg`.
function onColor(bg: string): string {
  return contrastRatio(bg, "#FFFFFF") >= contrastRatio(bg, "#101010")
    ? "#FFFFFF"
    : "#101010";
}

function swatch(main: string, lightMode: boolean): Swatch {
  const toward = lightMode ? "#FFFFFF" : "#000000";
  return {
    main,
    light: mix(main, toward, 0.25),
    dark: mix(main, lightMode ? "#000000" : "#FFFFFF", 0.3),
    contrastText: onColor(main),
  };
}

function derive(base: Base, lightMode: boolean): SchemeColors {
  const extreme = lightMode ? "#FFFFFF" : "#000000";
  const muted = base.mutedText ?? 0.64;
  const shade = (t: number) => mix(base.background, base.surface, t);
  const surfaceLow = shade(0.25);
  const primaryContainer = mix(
    base.paper,
    base.primary,
    lightMode ? 0.28 : 0.3,
  );
  const secondaryContainer = mix(
    base.paper,
    base.secondary,
    lightMode ? 0.28 : 0.3,
  );
  return {
    primary: swatch(base.primary, lightMode),
    secondary: swatch(base.secondary, lightMode),
    background: { default: base.background, paper: base.paper },
    text: {
      primary: base.text,
      secondary: rgba(base.text, muted),
    },
    tonal: {
      primaryContainer,
      onPrimaryContainer: mix(base.primary, base.text, 0.85),
      secondaryContainer,
      onSecondaryContainer: mix(base.secondary, base.text, 0.85),
      surfaceContainerLowest: mix(base.background, extreme, 0.7),
      surfaceContainerLow: surfaceLow,
      surfaceContainer: shade(0.5),
      surfaceContainerHigh: shade(0.75),
      surfaceContainerHighest: base.surface,
      outline: rgba(base.text, 0.18),
      outlineVariant: rgba(base.text, 0.1),
    },
    appBar: { background: rgba(surfaceLow, 0.9) },
  };
}

// ---------------------------------------------------------------------------
// The schemes.

const parchment: SchemeDef = {
  label: "Parchment",
  entryFont: SANS,
  light: {
    primary: {
      main: "#2F6F4E",
      light: "#5C9B79",
      dark: "#1D4E34",
      contrastText: "#FFFFFF",
    },
    secondary: {
      main: "#8A6100",
      light: "#B98A00",
      dark: "#5E4100",
      contrastText: "#FFFFFF",
    },
    background: { default: "#F4EFE2", paper: "#FBF8F2" },
    text: { primary: "#1E1B15", secondary: "rgba(30, 27, 21, 0.64)" },
    tonal: {
      primaryContainer: "#B7EFD1",
      onPrimaryContainer: "#04341F",
      secondaryContainer: "#FFE7B0",
      onSecondaryContainer: "#2A1D00",
      surfaceContainerLowest: "#FFFFFF",
      surfaceContainerLow: "#F7F2E9",
      surfaceContainer: "#F1EBDD",
      surfaceContainerHigh: "#EBE3D2",
      surfaceContainerHighest: "#E5DBC6",
      outline: "rgba(30, 26, 16, 0.18)",
      outlineVariant: "rgba(30, 26, 16, 0.10)",
    },
    appBar: { background: "rgba(247, 242, 233, 0.9)" },
  },
  dark: {
    primary: {
      main: "#8FD9AF",
      light: "#B7EFD1",
      dark: "#5C9B79",
      contrastText: "#08331E",
    },
    secondary: {
      main: "#F0C34C",
      light: "#F7D67A",
      dark: "#B98A00",
      contrastText: "#3D2B00",
    },
    background: { default: "#14120F", paper: "#181510" },
    text: { primary: "#E9E3D6", secondary: "rgba(233, 227, 214, 0.66)" },
    tonal: {
      primaryContainer: "#1D4E34",
      onPrimaryContainer: "#B7EFD1",
      secondaryContainer: "#4A3800",
      onSecondaryContainer: "#FFE7B0",
      surfaceContainerLowest: "#0C0B09",
      surfaceContainerLow: "#1C1915",
      surfaceContainer: "#211D18",
      surfaceContainerHigh: "#2B2620",
      surfaceContainerHighest: "#37312A",
      outline: "rgba(233, 227, 214, 0.20)",
      outlineVariant: "rgba(233, 227, 214, 0.10)",
    },
    appBar: { background: "rgba(28, 25, 21, 0.88)" },
  },
};

const slate: SchemeDef = {
  label: "Slate",
  entryFont: SANS,
  light: derive(
    {
      background: "#EEF1F5",
      paper: "#F8FAFC",
      text: "#1B2430",
      primary: "#2B5C9E",
      secondary: "#6B4FA0",
      surface: "#D5DCE6",
    },
    true,
  ),
  dark: derive(
    {
      background: "#11151B",
      paper: "#151A21",
      text: "#E3E8EF",
      primary: "#8DB8F0",
      secondary: "#C3B0F0",
      surface: "#2E3846",
    },
    false,
  ),
};

const ink: SchemeDef = {
  label: "Ink",
  entryFont: SERIF,
  light: derive(
    {
      background: "#F5F5F3",
      paper: "#FBFBFA",
      text: "#111111",
      primary: "#2A2A2A",
      secondary: "#6B6B6B",
      surface: "#DCDCD8",
    },
    true,
  ),
  dark: derive(
    {
      background: "#121212",
      paper: "#171717",
      text: "#ECECEC",
      primary: "#D0D0D0",
      secondary: "#9A9A9A",
      surface: "#353535",
    },
    false,
  ),
};

// Catppuccin Latte (light) and Mocha (dark): base/mantle as paper/background,
// surface1 as the darkest shade, mauve and blue as the accents.
const catppuccin: SchemeDef = {
  label: "Catppuccin",
  entryFont: SANS,
  light: derive(
    {
      background: "#E6E9EF",
      paper: "#EFF1F5",
      text: "#4C4F69",
      primary: "#8839EF",
      secondary: "#1E66F5",
      surface: "#CCD0DA",
      mutedText: 0.85,
    },
    true,
  ),
  dark: derive(
    {
      background: "#181825",
      paper: "#1E1E2E",
      text: "#CDD6F4",
      primary: "#CBA6F7",
      secondary: "#89B4FA",
      surface: "#45475A",
    },
    false,
  ),
};

const contrast: SchemeDef = {
  label: "High contrast",
  entryFont: SANS,
  light: derive(
    {
      background: "#FFFFFF",
      paper: "#FFFFFF",
      text: "#000000",
      primary: "#0033CC",
      secondary: "#8A2A00",
      surface: "#E0E0E0",
      mutedText: 0.8,
    },
    true,
  ),
  dark: derive(
    {
      background: "#000000",
      paper: "#000000",
      text: "#FFFFFF",
      primary: "#8FB3FF",
      secondary: "#FFB380",
      surface: "#333333",
      mutedText: 0.82,
    },
    false,
  ),
};

export const SCHEMES: Record<ColorScheme, SchemeDef> = {
  parchment,
  slate,
  ink,
  catppuccin,
  contrast,
};

export const SCHEME_ORDER: ColorScheme[] = [
  "parchment",
  "slate",
  "ink",
  "catppuccin",
  "contrast",
];
