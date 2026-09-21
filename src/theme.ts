import {
  createTheme,
  type PaletteMode,
  type Theme,
} from "@mui/material/styles";

declare module "@mui/material/styles" {
  interface Palette {
    tonal: {
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
    };
  }
  interface PaletteOptions {
    tonal: Palette["tonal"];
  }
}

const lightTonal = {
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
};

const darkTonal = {
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
};

export function getTheme(mode: PaletteMode): Theme {
  const isLight = mode === "light";
  const tonal = isLight ? lightTonal : darkTonal;

  return createTheme({
    palette: {
      mode,
      primary: isLight
        ? {
            main: "#2F6F4E",
            light: "#5C9B79",
            dark: "#1D4E34",
            contrastText: "#FFFFFF",
          }
        : {
            main: "#8FD9AF",
            light: "#B7EFD1",
            dark: "#5C9B79",
            contrastText: "#08331E",
          },
      secondary: isLight
        ? {
            main: "#8A6100",
            light: "#B98A00",
            dark: "#5E4100",
            contrastText: "#FFFFFF",
          }
        : {
            main: "#F0C34C",
            light: "#F7D67A",
            dark: "#B98A00",
            contrastText: "#3D2B00",
          },
      background: {
        default: isLight ? "#F4EFE2" : "#14120F",
        paper: isLight ? "#FBF8F2" : "#181510",
      },
      text: {
        primary: isLight ? "#1E1B15" : "#E9E3D6",
        secondary: isLight
          ? "rgba(30, 27, 21, 0.64)"
          : "rgba(233, 227, 214, 0.66)",
      },
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
            backgroundColor: isLight
              ? "rgba(247, 242, 233, 0.9)"
              : "rgba(28, 25, 21, 0.88)",
            color: isLight ? "#1E1B15" : "#E9E3D6",
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
