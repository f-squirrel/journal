import { useState, type ReactElement } from "react";
import { Badge, Box, IconButton, Stack, Tooltip } from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import BookIcon from "@mui/icons-material/AutoStories";
import CalendarIcon from "@mui/icons-material/CalendarMonthOutlined";
import CloudDoneIcon from "@mui/icons-material/CloudDone";
import CloudOffIcon from "@mui/icons-material/CloudOff";
import CloudSyncIcon from "@mui/icons-material/CloudSync";
import CloudUploadIcon from "@mui/icons-material/CloudUpload";
import DarkModeIcon from "@mui/icons-material/DarkMode";
import ExportIcon from "@mui/icons-material/FileDownload";
import KeyboardIcon from "@mui/icons-material/Keyboard";
import LightModeIcon from "@mui/icons-material/LightMode";
import LogoutIcon from "@mui/icons-material/Logout";
import SettingsIcon from "@mui/icons-material/SettingsOutlined";
import SyncProblemIcon from "@mui/icons-material/SyncProblem";
import { useMatch, useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import { useColorMode } from "../context/ColorModeContext";
import { useNewEntry } from "../hooks/useNewEntry";
import { useNow } from "../hooks/useNow";
import { ariaKeyshortcuts, formatBinding } from "../shortcuts/binding";
import { useShortcuts } from "../shortcuts/ShortcutsContext";
import type { SyncState } from "../types";
import { onThisDay } from "../utils/date";
import { syncSummary } from "../utils/sync";
import ExportDialog from "./ExportDialog";
import SyncDialog from "./SyncDialog";
import { HEADER_HEIGHT, SIDEBAR_WIDTH, rem } from "./chrome";

/// One rail button: an icon, and the label only on hover. The label is also
/// the accessible name, so the button is still addressable by the name a user
/// reads ("New entry", "Log out") even though nothing is drawn beside it.
function RailButton({
  label,
  icon,
  onClick,
  filled,
  active,
  badge,
  tone,
  testId,
  shortcut,
}: {
  label: string;
  icon: ReactElement;
  onClick: () => void;
  filled?: boolean;
  /// The view this button opens is the one on screen. A quieter mark than
  /// `filled`, which belongs to "New entry".
  active?: boolean;
  /// A dot on the icon: there is something behind this button worth a look.
  badge?: boolean;
  /// Colours the icon for a state that needs the user; plain otherwise.
  tone?: "warning" | "error";
  testId?: string;
  /// The chord that triggers this button's action, if any — shown muted
  /// beside the label in the tooltip, and reported to assistive tech through
  /// `aria-keyshortcuts`. `aria-label` stays exactly `label` either way: the
  /// e2e suite finds these buttons by that name.
  shortcut?: string | null;
}) {
  return (
    <Tooltip
      title={
        shortcut ? (
          <Stack direction="row" alignItems="baseline" spacing={0.5}>
            <span>{label}</span>
            <Box component="span" sx={{ opacity: 0.7, fontSize: rem(11) }}>
              {formatBinding(shortcut)}
            </Box>
          </Stack>
        ) : (
          label
        )
      }
      placement="right"
      arrow
    >
      <IconButton
        aria-label={label}
        aria-current={active ? "page" : undefined}
        aria-keyshortcuts={ariaKeyshortcuts(shortcut ?? null)}
        data-testid={testId}
        onClick={onClick}
        sx={{
          borderRadius: 2,
          color: filled
            ? "tonal.onPrimaryContainer"
            : tone
              ? `${tone}.main`
              : "text.secondary",
          bgcolor: filled
            ? "tonal.primaryContainer"
            : active
              ? "action.selected"
              : "transparent",
          "&:hover": {
            bgcolor: filled ? "tonal.primaryContainer" : "action.hover",
            ...(filled && { filter: "brightness(0.96)" }),
          },
        }}
      >
        {badge ? (
          <Badge color="primary" variant="dot">
            {icon}
          </Badge>
        ) : (
          icon
        )}
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

/// The rail's calendar button. Its label carries the one thing worth knowing
/// before opening it — how many entries the journal holds for today's date in
/// earlier years — and a dot on the icon says the same to the eye, so the
/// feature shows itself on the day it has something to show.
function CalendarButton({
  onClick,
  shortcut,
}: {
  onClick: () => void;
  shortcut?: string | null;
}) {
  const { entries } = useAppState();
  const active = useMatch("/calendar") !== null;
  // Re-read each minute so the dot survives midnight in a window left open.
  const now = useNow(true, 60_000);
  const count = onThisDay(entries, new Date(now)).reduce(
    (total, group) => total + group.entries.length,
    0,
  );
  return (
    <RailButton
      label={
        count === 0
          ? "Calendar"
          : `Calendar — ${count} ${count === 1 ? "entry" : "entries"} on this day`
      }
      icon={<CalendarIcon fontSize="small" />}
      active={active}
      badge={count > 0}
      onClick={onClick}
      shortcut={shortcut}
    />
  );
}

/// The leftmost column: the journal's identity, its views, and the session
/// controls, collapsed to an icon rail one button wide — each button names
/// itself in a tooltip on hover. It lists no Media/Map rows, because there is
/// nothing behind them yet.
export default function Sidebar({
  onNavigate,
  onShowShortcuts,
}: {
  onNavigate?: () => void;
  /// Opens `ShortcutsDialog`, which `JournalLayout` owns — the sidebar itself
  /// is unmounted while its drawer is closed on phones, so it can't hold that
  /// state.
  onShowShortcuts: () => void;
}) {
  const { logout } = useAppState();
  const { mode, toggleMode } = useColorMode();
  const navigate = useNavigate();
  const newEntry = useNewEntry();
  const { bindingFor } = useShortcuts();
  const [syncOpen, setSyncOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const settingsActive = useMatch("/settings") !== null;

  return (
    <Stack
      alignItems="center"
      data-testid="sidebar"
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
          shortcut={bindingFor("entry.new")}
        />
        <CalendarButton
          onClick={() => {
            navigate("/calendar");
            onNavigate?.();
          }}
          shortcut={bindingFor("view.calendar")}
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
            label="Keyboard shortcuts"
            icon={<KeyboardIcon fontSize="small" />}
            onClick={onShowShortcuts}
            shortcut={bindingFor("help.shortcuts")}
          />
          <RailButton
            label="Settings"
            icon={<SettingsIcon fontSize="small" />}
            active={settingsActive}
            onClick={() => {
              navigate("/settings");
              onNavigate?.();
            }}
            shortcut={bindingFor("view.settings")}
          />
          <RailButton
            label="Log out"
            icon={<LogoutIcon fontSize="small" />}
            onClick={() => {
              logout();
              navigate("/login");
            }}
            shortcut={bindingFor("vault.lock")}
          />
        </Stack>
      </Box>
      <SyncDialog open={syncOpen} onClose={() => setSyncOpen(false)} />
      <ExportDialog open={exportOpen} onClose={() => setExportOpen(false)} />
    </Stack>
  );
}
