import { useMemo, type ReactNode } from "react";
import { CssBaseline, ThemeProvider } from "@mui/material";
import { LocalizationProvider } from "@mui/x-date-pickers/LocalizationProvider";
import { AdapterDateFns } from "@mui/x-date-pickers/AdapterDateFns";
import { enUS } from "date-fns/locale";
import {
  HashRouter,
  Navigate,
  Route,
  Routes,
  useParams,
} from "react-router-dom";
import { getTheme } from "./theme";
import { AppStateProvider, useAppState } from "./context/AppStateContext";
import { ColorModeProvider, useColorMode } from "./context/ColorModeContext";
import { SettingsProvider, useSettings } from "./context/SettingsContext";
import { ShortcutsProvider } from "./shortcuts/ShortcutsContext";
import JournalLayout from "./components/JournalLayout";
import CalendarPage from "./pages/CalendarPage";
import LoginPage from "./pages/LoginPage";
import SettingsPage from "./pages/SettingsPage";
import EntryPaneRoute, { NoEntrySelected } from "./pages/EntryPane";

function RequireAuth({ children }: { children: ReactNode }): ReactNode {
  const { authenticated } = useAppState();
  return authenticated ? children : <Navigate to="/login" replace />;
}

function NotFoundRedirect() {
  const { authenticated } = useAppState();
  return <Navigate to={authenticated ? "/entries" : "/login"} replace />;
}

/// Editing happens in the entry pane now, so the old editor route has no page
/// of its own. It redirects rather than falling through to `NotFoundRedirect`,
/// which would drop whoever followed a bookmark back at an empty list.
function RedirectToEntry() {
  const { id } = useParams<{ id: string }>();
  return <Navigate to={id ? `/entries/${id}` : "/entries"} replace />;
}

function AppRoutes() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      {/* One layout for every authenticated route: the sidebar and the entry
          list mount once and survive selection, so the list keeps its scroll
          position as entries are opened. */}
      <Route
        element={
          <RequireAuth>
            <JournalLayout />
          </RequireAuth>
        }
      >
        <Route path="/entries" element={<NoEntrySelected />} />
        <Route path="/entries/:id" element={<EntryPaneRoute />} />
        <Route path="/calendar" element={<CalendarPage />} />
        <Route path="/settings" element={<SettingsPage />} />
      </Route>
      <Route path="/entries/:id/edit" element={<RedirectToEntry />} />
      <Route path="*" element={<NotFoundRedirect />} />
    </Routes>
  );
}

/// `week_starts_on` as the numeral date-fns' `enUS` locale wants (Sunday 0,
/// Monday 1, Saturday 6), which is how the calendar grid picks up the
/// journal setting: `LocalizationProvider`'s `adapterLocale` is the only knob
/// `AdapterDateFns` exposes for it.
function weekStartsOnNumber(
  setting: "sunday" | "monday" | "saturday",
): 0 | 1 | 6 {
  switch (setting) {
    case "monday":
      return 1;
    case "saturday":
      return 6;
    case "sunday":
      return 0;
  }
}

function ThemedApp() {
  const { mode } = useColorMode();
  const { settings } = useSettings();
  const weekStartsOn = weekStartsOnNumber(settings.week_starts_on);
  // `enUS` plus the one option this app ever changes on it. Recomputed only
  // when the setting itself changes, so the calendar isn't handed a fresh
  // locale object — and remounted — on every unrelated render.
  const adapterLocale = useMemo(
    () => ({ ...enUS, options: { ...enUS.options, weekStartsOn } }),
    [weekStartsOn],
  );

  return (
    <ThemeProvider theme={getTheme(mode)}>
      <CssBaseline />
      {/* The entry pane's date picker needs this. date-fns is the adapter
          because it is the one `@mui/x-date-pickers` supports that is plain
          `Date` in and out — the app's timestamps are ISO strings, so no other
          date type ever enters the codebase. `adapterLocale` carries the
          journal's week-start setting (see the plan, WP5). */}
      <LocalizationProvider
        dateAdapter={AdapterDateFns}
        adapterLocale={adapterLocale}
      >
        <AppStateProvider>
          <ShortcutsProvider>
            <HashRouter>
              <AppRoutes />
            </HashRouter>
          </ShortcutsProvider>
        </AppStateProvider>
      </LocalizationProvider>
    </ThemeProvider>
  );
}

export default function App() {
  return (
    <SettingsProvider>
      <ColorModeProvider>
        <ThemedApp />
      </ColorModeProvider>
    </SettingsProvider>
  );
}
