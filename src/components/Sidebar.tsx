import { useState, type ReactElement } from "react";
import { Box, IconButton, Stack, Tooltip } from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import BookIcon from "@mui/icons-material/AutoStories";
import CloudDoneIcon from "@mui/icons-material/CloudDone";
import CloudOffIcon from "@mui/icons-material/CloudOff";
import CloudSyncIcon from "@mui/icons-material/CloudSync";
import CloudUploadIcon from "@mui/icons-material/CloudUpload";
import DarkModeIcon from "@mui/icons-material/DarkMode";
import ExportIcon from "@mui/icons-material/FileDownload";
import LightModeIcon from "@mui/icons-material/LightMode";
import LogoutIcon from "@mui/icons-material/Logout";
import SyncProblemIcon from "@mui/icons-material/SyncProblem";
import { useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import { useColorMode } from "../context/ColorModeContext";
import { useNewEntry } from "../hooks/useNewEntry";
import { useNow } from "../hooks/useNow";
import type { SyncState } from "../types";
import { syncSummary } from "../utils/sync";
import ExportDialog from "./ExportDialog";
import SyncDialog from "./SyncDialog";
import { HEADER_HEIGHT, SIDEBAR_WIDTH } from "./chrome";

/// One rail button: an icon, and the label only on hover. The label is also
/// the accessible name, so the button is still addressable by the name a user
/// reads ("New entry", "Log out") even though nothing is drawn beside it.
function RailButton({
  label,
  icon,
  onClick,
  filled,
  tone,
  testId,
}: {
  label: string;
  icon: ReactElement;
  onClick: () => void;
  filled?: boolean;
  /// Colours the icon for a state that needs the user; plain otherwise.
  tone?: "warning" | "error";
  testId?: string;
}) {
  return (
    <Tooltip title={label} placement="right" arrow>
      <IconButton
        aria-label={label}
        data-testid={testId}
        onClick={onClick}
        sx={{
          borderRadius: 2,
          color: filled
            ? "tonal.onPrimaryContainer"
            : tone
              ? `${tone}.main`
              : "text.secondary",
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

/// The icon that says what sync is doing, and the tone it takes when the user
/// has something to do about it.
function syncAppearance(state: SyncState): {
  icon: ReactElement;
  tone?: "warning" | "error";
} {
  const small = { fontSize: "small" } as const;
  switch (state) {
    case "off":
    case "offline":
      return { icon: <CloudOffIcon {...small} /> };
    case "synced":
      return { icon: <CloudDoneIcon {...small} /> };
    case "pending":
      return { icon: <CloudUploadIcon {...small} /> };
    case "syncing":
      return { icon: <CloudSyncIcon {...small} /> };
    case "conflicts":
      return { icon: <SyncProblemIcon {...small} />, tone: "warning" };
    case "error":
    case "reauth":
    case "key_mismatch":
      return { icon: <SyncProblemIcon {...small} />, tone: "error" };
  }
}

/// The rail's sync button. Its label is the status itself ("2 changes not
/// uploaded yet"), which is also its accessible name, and pressing it opens the
/// dialog where the account is connected, disconnected and retried.
function SyncButton({ onClick }: { onClick: () => void }) {
  const { sync } = useAppState();
  // Only a "synced · 5 min ago" label goes stale on its own.
  const now = useNow(sync.state === "synced");
  const { icon, tone } = syncAppearance(sync.state);
  return (
    <RailButton
      label={syncSummary(sync, now)}
      icon={icon}
      tone={tone}
      testId="sync-button"
      onClick={onClick}
    />
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
  const [syncOpen, setSyncOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);

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
          <SyncButton onClick={() => setSyncOpen(true)} />
          <RailButton
            label="Export journal"
            icon={<ExportIcon fontSize="small" />}
            onClick={() => setExportOpen(true)}
          />
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
      <SyncDialog open={syncOpen} onClose={() => setSyncOpen(false)} />
      <ExportDialog open={exportOpen} onClose={() => setExportOpen(false)} />
    </Stack>
  );
}
