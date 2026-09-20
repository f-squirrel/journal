import type { ReactNode } from "react";
import { CssBaseline, ThemeProvider } from "@mui/material";
import { LocalizationProvider } from "@mui/x-date-pickers/LocalizationProvider";
import { AdapterDateFns } from "@mui/x-date-pickers/AdapterDateFns";
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
import JournalLayout from "./components/JournalLayout";
import CalendarPage from "./pages/CalendarPage";
import LoginPage from "./pages/LoginPage";
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
      </Route>
      <Route path="/entries/:id/edit" element={<RedirectToEntry />} />
      <Route path="*" element={<NotFoundRedirect />} />
    </Routes>
  );
}

function ThemedApp() {
  const { mode } = useColorMode();

  return (
    <ThemeProvider theme={getTheme(mode)}>
      <CssBaseline />
      {/* The entry pane's date picker needs this. date-fns is the adapter
          because it is the one `@mui/x-date-pickers` supports that is plain
          `Date` in and out — the app's timestamps are ISO strings, so no other
          date type ever enters the codebase. */}
      <LocalizationProvider dateAdapter={AdapterDateFns}>
        <AppStateProvider>
          <HashRouter>
            <AppRoutes />
          </HashRouter>
        </AppStateProvider>
      </LocalizationProvider>
    </ThemeProvider>
  );
}

export default function App() {
  return (
    <ColorModeProvider>
      <ThemedApp />
    </ColorModeProvider>
  );
}
