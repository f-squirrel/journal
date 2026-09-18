import type { ReactElement } from "react";
import { Box, IconButton, Stack, Tooltip } from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import BookIcon from "@mui/icons-material/AutoStories";
import DarkModeIcon from "@mui/icons-material/DarkMode";
import LightModeIcon from "@mui/icons-material/LightMode";
import LogoutIcon from "@mui/icons-material/Logout";
import { useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import { useColorMode } from "../context/ColorModeContext";
import { useNewEntry } from "../hooks/useNewEntry";
import { HEADER_HEIGHT, SIDEBAR_WIDTH } from "./chrome";

/// One rail button: an icon, and the label only on hover. The label is also
/// the accessible name, so the button is still addressable by the name a user
/// reads ("New entry", "Log out") even though nothing is drawn beside it.
function RailButton({
  label,
  icon,
  onClick,
  filled,
}: {
  label: string;
  icon: ReactElement;
  onClick: () => void;
  filled?: boolean;
}) {
  return (
    <Tooltip title={label} placement="right" arrow>
      <IconButton
        aria-label={label}
        onClick={onClick}
        sx={{
          borderRadius: 2,
          color: filled ? "tonal.onPrimaryContainer" : "text.secondary",
          bgcolor: filled ? "tonal.primaryContainer" : "transparent",
          "&:hover": {
            bgcolor: filled ? "tonal.primaryContainer" : "action.hover",
            ...(filled && { filter: "brightness(0.96)" }),
          },
        }}
      >
        {icon}
      </IconButton>
    </Tooltip>
  );
}

/// The leftmost column: the journal's identity, the one view there is, and the
/// session controls, collapsed to an icon rail one button wide — each button
/// names itself in a tooltip on hover. It lists no Calendar/Media/Map rows,
/// because there is nothing behind them yet.
export default function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const { logout } = useAppState();
  const { mode, toggleMode } = useColorMode();
  const navigate = useNavigate();
  const newEntry = useNewEntry();

  return (
    <Stack
      alignItems="center"
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
        alignItems="center"
        justifyContent="center"
        sx={{
          width: "100%",
          height: HEADER_HEIGHT,
          flexShrink: 0,
          borderBottom: 1,
          borderColor: "divider",
        }}
      >
        <Tooltip title="Journal" placement="right" arrow>
          <BookIcon color="primary" fontSize="small" />
        </Tooltip>
      </Stack>

      <Stack alignItems="center" spacing={0.5} sx={{ flex: 1, pt: 1.5 }}>
        <RailButton
          label="New entry"
          filled
          icon={<AddIcon fontSize="small" />}
          onClick={() => {
            newEntry();
            onNavigate?.();
          }}
        />
      </Stack>

      <Box sx={{ width: "100%", borderTop: 1, borderColor: "divider", py: 1 }}>
        <Stack alignItems="center" spacing={0.5}>
          <RailButton
            label={mode === "dark" ? "Light mode" : "Dark mode"}
            icon={
              mode === "dark" ? (
                <LightModeIcon fontSize="small" />
              ) : (
                <DarkModeIcon fontSize="small" />
              )
            }
            onClick={toggleMode}
          />
          <RailButton
            label="Log out"
            icon={<LogoutIcon fontSize="small" />}
            onClick={() => {
              logout();
              navigate("/login");
            }}
          />
        </Stack>
      </Box>
    </Stack>
  );
}
