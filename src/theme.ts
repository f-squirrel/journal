import {
  createTheme,
  type PaletteMode,
  type Theme,
} from "@mui/material/styles";

import { SCHEMES, type Tonal } from "./schemes";
import type { ColorScheme } from "./types";

declare module "@mui/material/styles" {
  interface Palette {
    tonal: Tonal;
  }
  interface PaletteOptions {
    tonal: Tonal;
  }
  interface TypographyVariants {
    /// Font stack for entry text; the scheme picks it (`src/schemes.ts`).
    entryFontFamily: string;
  }
  interface TypographyVariantsOptions {
    entryFontFamily?: string;
  }
}

export function getTheme(mode: PaletteMode, scheme: ColorScheme): Theme {
  const def = SCHEMES[scheme];
  const colors = def[mode];
  const { tonal } = colors;

  return createTheme({
    palette: {
      mode,
      primary: colors.primary,
      secondary: colors.secondary,
      background: colors.background,
      text: colors.text,
      divider: tonal.outlineVariant,
      tonal,
    },
    // Every gap, pad and margin in the app is `theme.spacing(n)`, so this one
    // line is what makes the whole layout track the reader's text size: MUI's
    // default is a flat 8px, this is the same 8px expressed in `rem`. Type
    // that grows without its whitespace growing with it only gets cramped.
    spacing: (factor: number) => `${factor * 0.5}rem`,
    // Radii stay in `px`: a corner is a corner at any text size, and rounding
    // that grows with the font turns a button into a pill.
    shape: { borderRadius: 10 },
    typography: {
      fontFamily: '"Roboto", "Helvetica Neue", Arial, sans-serif',
      entryFontFamily: def.entryFont,
      // Tracking in `em`, so it stays proportional to the size it lands on
      // rather than being a fixed pixel nudge that over-tightens large text
      // and under-tightens small.
      h4: { fontWeight: 700, letterSpacing: "-0.015em" },
      h5: { fontWeight: 700, letterSpacing: "-0.0125em" },
      h6: { fontWeight: 700, letterSpacing: "-0.0125em" },
      button: { fontWeight: 600, textTransform: "none" },
    },
    components: {
      MuiCssBaseline: {
        styleOverrides: {
          // The shell fills the viewport and scrolls its columns separately,
          // so the chain down to #root has to have a height to fill.
          "html, body, #root": { height: "100%" },
        },
      },
      MuiAppBar: {
        styleOverrides: {
          root: {
            backdropFilter: "blur(10px)",
            backgroundColor: colors.appBar.background,
            color: colors.text.primary,
          },
        },
      },
      MuiButton: {
        styleOverrides: {
          root: { borderRadius: 100, paddingInline: "1.25rem" },
          contained: { boxShadow: "none", "&:hover": { boxShadow: "none" } },
        },
      },
      MuiFab: {
        styleOverrides: {
          root: {
            backgroundColor: tonal.primaryContainer,
            color: tonal.onPrimaryContainer,
            boxShadow: "none",
            "&:hover": {
              backgroundColor: tonal.primaryContainer,
              boxShadow: "none",
              filter: "brightness(0.96)",
            },
          },
        },
      },
      MuiPaper: {
        styleOverrides: {
          root: { backgroundImage: "none" },
        },
      },
      MuiCard: {
        styleOverrides: {
          root: {
            borderRadius: 18,
            backgroundColor: tonal.surfaceContainerLow,
          },
        },
      },
      MuiTextField: {
        defaultProps: { variant: "outlined" },
      },
      MuiListItemButton: {
        styleOverrides: {
          root: { borderRadius: 8 },
        },
      },
      MuiChip: {
        styleOverrides: {
          root: { fontWeight: 500 },
        },
      },
    },
  });
}
