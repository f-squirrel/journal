import {
  Box,
  Button,
  List,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  Stack,
  Typography,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import BookIcon from "@mui/icons-material/AutoStories";
import DarkModeIcon from "@mui/icons-material/DarkMode";
import LightModeIcon from "@mui/icons-material/LightMode";
import LogoutIcon from "@mui/icons-material/Logout";
import NotesIcon from "@mui/icons-material/Notes";
import { useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import { useColorMode } from "../context/ColorModeContext";
import { useNewEntry } from "../hooks/useNewEntry";
import { HEADER_HEIGHT, SIDEBAR_WIDTH } from "./chrome";

/// The leftmost column: the journal's identity, the one view there is, and the
/// session controls. It lists no Calendar/Media/Map rows, because there is
/// nothing behind them yet.
export default function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const { entries, logout } = useAppState();
  const { mode, toggleMode } = useColorMode();
  const navigate = useNavigate();
  const newEntry = useNewEntry();

  return (
    <Stack
      sx={{
        width: SIDEBAR_WIDTH,
        flexShrink: 0,
        height: "100%",
        bgcolor: "tonal.surfaceContainer",
        borderRight: 1,
        borderColor: "divider",
      }}
    >
      <Stack
        direction="row"
        spacing={1}
        alignItems="center"
        sx={{
          px: 2,
          height: HEADER_HEIGHT,
          flexShrink: 0,
          borderBottom: 1,
          borderColor: "divider",
        }}
      >
        <BookIcon color="primary" fontSize="small" />
        <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
          Journal
        </Typography>
      </Stack>

      <Button
        variant="contained"
        disableElevation
        fullWidth
        startIcon={<AddIcon />}
        onClick={() => {
          newEntry();
          onNavigate?.();
        }}
        sx={{ mt: 1.5, mx: 1.5, mb: 2, width: "auto" }}
      >
        New entry
      </Button>

      <List disablePadding sx={{ flex: 1, px: 1.5 }}>
        <ListItemButton
          selected
          onClick={() => {
            navigate("/entries");
            onNavigate?.();
          }}
        >
          <ListItemIcon sx={{ minWidth: 34 }}>
            <NotesIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText primary="All entries" primaryTypographyProps={{ fontSize: 14, fontWeight: 600 }} />
          <Typography variant="caption" color="text.secondary">
            {entries.length}
          </Typography>
        </ListItemButton>
      </List>

      <Box sx={{ borderTop: 1, borderColor: "divider", p: 1.5 }}>
        <List disablePadding>
          <ListItemButton onClick={toggleMode}>
            <ListItemIcon sx={{ minWidth: 34 }}>
              {mode === "dark" ? <LightModeIcon fontSize="small" /> : <DarkModeIcon fontSize="small" />}
            </ListItemIcon>
            <ListItemText
              primary={mode === "dark" ? "Light mode" : "Dark mode"}
              primaryTypographyProps={{ fontSize: 14 }}
            />
          </ListItemButton>
          <ListItemButton
            onClick={() => {
              logout();
              navigate("/login");
            }}
          >
            <ListItemIcon sx={{ minWidth: 34 }}>
              <LogoutIcon fontSize="small" />
            </ListItemIcon>
            <ListItemText primary="Log out" primaryTypographyProps={{ fontSize: 14 }} />
          </ListItemButton>
        </List>
      </Box>
    </Stack>
  );
}
