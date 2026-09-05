import type { ReactNode } from "react";
import { CssBaseline, ThemeProvider } from "@mui/material";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { getTheme } from "./theme";
import { AppStateProvider, useAppState } from "./context/AppStateContext";
import { ColorModeProvider, useColorMode } from "./context/ColorModeContext";
import LoginPage from "./pages/LoginPage";
import EntryListPage from "./pages/EntryListPage";
import PreviewPage from "./pages/PreviewPage";
import EditorPage from "./pages/EditorPage";

function RequireAuth({ children }: { children: ReactNode }): ReactNode {
  const { authenticated } = useAppState();
  return authenticated ? children : <Navigate to="/login" replace />;
}

function NotFoundRedirect() {
  const { authenticated } = useAppState();
  return <Navigate to={authenticated ? "/entries" : "/login"} replace />;
}

function AppRoutes() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        path="/entries"
        element={
          <RequireAuth>
            <EntryListPage />
          </RequireAuth>
        }
      />
      <Route
        path="/entries/:id"
        element={
          <RequireAuth>
            <PreviewPage />
          </RequireAuth>
        }
      />
      <Route
        path="/entries/:id/edit"
        element={
          <RequireAuth>
            <EditorPage />
          </RequireAuth>
        }
      />
      <Route path="*" element={<NotFoundRedirect />} />
    </Routes>
  );
}

function ThemedApp() {
  const { mode } = useColorMode();

  return (
    <ThemeProvider theme={getTheme(mode)}>
      <CssBaseline />
      <AppStateProvider>
        <HashRouter>
          <AppRoutes />
        </HashRouter>
      </AppStateProvider>
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
