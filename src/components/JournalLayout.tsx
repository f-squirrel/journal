import { useState } from "react";
import { Box, Drawer, useMediaQuery, useTheme } from "@mui/material";
import { Outlet, useMatch } from "react-router-dom";
import EntryList from "./EntryList";
import Sidebar from "./Sidebar";

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
  // Two hooks, not `a || b`: a short-circuit would skip the second on some
  // renders and break the rules of hooks.
  const entryMatch = useMatch("/entries/:id");
  const calendarMatch = useMatch("/calendar");
  const paneOpen = entryMatch !== null || calendarMatch !== null;

  return (
    <Box sx={{ display: "flex", height: "100%", overflow: "hidden" }}>
      {compact ? (
        <Drawer open={drawerOpen} onClose={() => setDrawerOpen(false)}>
          <Sidebar onNavigate={() => setDrawerOpen(false)} />
        </Drawer>
      ) : (
        <Sidebar />
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
  );
}
