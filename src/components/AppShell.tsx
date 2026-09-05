import type { ReactNode } from "react";
import {
  AppBar,
  Box,
  Button,
  IconButton,
  Stack,
  Tooltip,
  Toolbar,
  Typography,
} from "@mui/material";
import BookIcon from "@mui/icons-material/AutoStories";
import AddIcon from "@mui/icons-material/Add";
import LogoutIcon from "@mui/icons-material/Logout";
import LightModeIcon from "@mui/icons-material/LightMode";
import DarkModeIcon from "@mui/icons-material/DarkMode";
import { useLocation, useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import { useColorMode } from "../context/ColorModeContext";

export default function AppShell({ children }: { children: ReactNode }) {
  const navigate = useNavigate();
  const location = useLocation();
  const { logout } = useAppState();
  const { mode, toggleMode } = useColorMode();

  const isEntries = location.pathname === "/entries";

  return (
    <Box sx={{ display: "flex", flexDirection: "column", minHeight: "100vh" }}>
      <AppBar position="sticky" elevation={0} sx={{ borderBottom: 1, borderColor: "divider" }}>
        <Toolbar sx={{ gap: 1 }}>
          <Stack direction="row" spacing={1} alignItems="center" sx={{ mr: 1 }}>
            <BookIcon color="primary" />
            <Typography variant="h6" sx={{ fontWeight: 700 }}>
              Journal
            </Typography>
          </Stack>

          <Stack direction="row" spacing={0.5} sx={{ flex: 1 }}>
            <Button
              onClick={() => navigate("/entries")}
              sx={{
                bgcolor: isEntries ? "tonal.primaryContainer" : "transparent",
                color: isEntries ? "tonal.onPrimaryContainer" : "text.primary",
                "&:hover": {
                  bgcolor: isEntries ? "tonal.primaryContainer" : "action.hover",
                },
              }}
            >
              Entries
            </Button>
          </Stack>

          <Button
            variant="contained"
            disableElevation
            startIcon={<AddIcon />}
            onClick={() => navigate("/entries/new/edit")}
            sx={{ display: { xs: "none", sm: "inline-flex" } }}
          >
            New entry
          </Button>
          <Tooltip title="New entry">
            <IconButton
              onClick={() => navigate("/entries/new/edit")}
              sx={{
                display: { xs: "inline-flex", sm: "none" },
                bgcolor: "tonal.primaryContainer",
                color: "tonal.onPrimaryContainer",
              }}
            >
              <AddIcon />
            </IconButton>
          </Tooltip>

          <Tooltip title={mode === "dark" ? "Switch to light mode" : "Switch to dark mode"}>
            <IconButton onClick={toggleMode} sx={{ ml: 0.5 }}>
              {mode === "dark" ? <LightModeIcon fontSize="small" /> : <DarkModeIcon fontSize="small" />}
            </IconButton>
          </Tooltip>

          <Tooltip title="Log out">
            <IconButton
              onClick={() => {
                logout();
                navigate("/login");
              }}
              sx={{
                ml: 0.5,
                bgcolor: "tonal.secondaryContainer",
                color: "tonal.onSecondaryContainer",
                "&:hover": { bgcolor: "tonal.secondaryContainer", filter: "brightness(0.95)" },
              }}
            >
              <LogoutIcon fontSize="small" />
            </IconButton>
          </Tooltip>
        </Toolbar>
      </AppBar>
      <Box component="main" sx={{ flex: 1, p: { xs: 2, sm: 3, md: 4 }, overflowY: "auto" }}>
        {children}
      </Box>
    </Box>
  );
}
