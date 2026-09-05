import { createTheme, type PaletteMode, type Theme } from "@mui/material/styles";

export function getTheme(mode: PaletteMode): Theme {
  return createTheme({
    palette: {
      mode,
      primary: { main: "#396cd8" },
    },
    shape: { borderRadius: 10 },
  });
}
