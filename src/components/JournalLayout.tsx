import { useState } from "react";
import { Box, Drawer, useMediaQuery, useTheme } from "@mui/material";
import { Outlet, useMatch, useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import { useNewEntry } from "../hooks/useNewEntry";
import { useShortcut } from "../shortcuts/ShortcutsContext";
import EntryList from "./EntryList";
import Sidebar from "./Sidebar";
import ShortcutsDialog from "./ShortcutsDialog";

/// The whole app, on one screen: journal sidebar, entry list, and a pane that
/// holds an entry or the calendar.
///
/// It is a layout route rather than chrome each page renders for itself, so
/// the list mounts once and keeps its scroll position as entries are opened.
///
/// Below `md` — every phone, and Tauri ships to two of them — the three
/// columns become one: the sidebar moves into a drawer, and the list gives way
/// to the pane when an entry or the calendar is open. The panes are rendered
/// conditionally rather than hidden with CSS, so nothing off-screen keeps
/// running.
export default function JournalLayout() {
  const theme = useTheme();
  const compact = useMediaQuery(theme.breakpoints.down("md"));
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  // Two hooks, not `a || b`: a short-circuit would skip the second on some
  // renders and break the rules of hooks.
  const entryMatch = useMatch("/entries/:id");
  const calendarMatch = useMatch("/calendar");
  const settingsMatch = useMatch("/settings");
  const paneOpen =
    entryMatch !== null || calendarMatch !== null || settingsMatch !== null;

  const { logout } = useAppState();
  const navigate = useNavigate();
  const newEntry = useNewEntry();

  // Mounted for every authenticated route and nothing else, so this is the
  // one place a shortcut that must always work — regardless of whether the
  // sidebar or the list happen to be mounted right now — can register.
  useShortcut("entry.new", () => {
    setDrawerOpen(false);
    newEntry();
  });
  useShortcut("vault.lock", () => {
    setDrawerOpen(false);
    logout();
    navigate("/login");
  });
  useShortcut("view.calendar", () => {
    setDrawerOpen(false);
    navigate("/calendar");
  });
  useShortcut("view.settings", () => {
    setDrawerOpen(false);
    navigate("/settings");
  });
  useShortcut("help.shortcuts", () => {
    setDrawerOpen(false);
    setShortcutsOpen(true);
  });

  return (
    <>
      <Box sx={{ display: "flex", height: "100%", overflow: "hidden" }}>
        {compact ? (
          <Drawer open={drawerOpen} onClose={() => setDrawerOpen(false)}>
            <Sidebar
              onNavigate={() => setDrawerOpen(false)}
              onShowShortcuts={() => {
                setDrawerOpen(false);
                setShortcutsOpen(true);
              }}
            />
          </Drawer>
        ) : (
          <Sidebar onShowShortcuts={() => setShortcutsOpen(true)} />
        )}

        {(!compact || !paneOpen) && (
          <EntryList
            onOpenSidebar={compact ? () => setDrawerOpen(true) : undefined}
          />
        )}

        {(!compact || paneOpen) && (
          <Box
            sx={{
              flex: 1,
              minWidth: 0,
              height: "100%",
              bgcolor: "background.paper",
            }}
          >
            <Outlet />
          </Box>
        )}
      </Box>
      <ShortcutsDialog
        open={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
      />
    </>
  );
}
