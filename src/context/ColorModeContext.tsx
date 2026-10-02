import { createContext, useContext, useMemo, type ReactNode } from "react";
import { useMediaQuery, type PaletteMode } from "@mui/material";
import type { ColorMode, ColorScheme } from "../types";
import { useSettings } from "./SettingsContext";

// Colour mode is now a device setting (`color_mode`), not
// `localStorage["journal.colorMode"]`: see the plan, D3. `system` resolves
// here, through the OS media query, rather than in the backend, which has no
// notion of it.

interface ColorModeContextValue {
  /// What the theme should actually use: `system` already resolved.
  mode: PaletteMode;
  /// The stored three-way value, for the Settings page.
  setting: ColorMode;
  /// The named palette and reading font (`src/schemes.ts`).
  scheme: ColorScheme;
  /// Writes an explicit `light` or `dark` — never `system` — so a manual
  /// toggle always has a visible effect regardless of the OS setting.
  toggleMode: () => void;
}

const ColorModeContext = createContext<ColorModeContextValue | null>(null);

export function ColorModeProvider({ children }: { children: ReactNode }) {
  const { settings, update } = useSettings();
  const prefersDark = useMediaQuery("(prefers-color-scheme: dark)");
  const setting = settings.color_mode;
  const scheme = settings.color_scheme;
  const mode: PaletteMode =
    setting === "system" ? (prefersDark ? "dark" : "light") : setting;

  const value = useMemo<ColorModeContextValue>(
    () => ({
      mode,
      setting,
      scheme,
      toggleMode: () => {
        // Fire-and-forget: a refusal re-fetches inside `SettingsContext` and
        // the rail button reverts on its own once `settings` updates.
        update({ color_mode: mode === "dark" ? "light" : "dark" }).catch(() => {
          // Nothing to do here; see above.
        });
      },
    }),
    [mode, setting, scheme, update],
  );

  return (
    <ColorModeContext.Provider value={value}>
      {children}
    </ColorModeContext.Provider>
  );
}

export function useColorMode(): ColorModeContextValue {
  const ctx = useContext(ColorModeContext);
  if (!ctx) {
    throw new Error("useColorMode must be used within ColorModeProvider");
  }
  return ctx;
}
