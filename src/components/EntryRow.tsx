import { Box, ListItemButton, Stack, Tooltip, Typography } from "@mui/material";
import CloudUploadOutlinedIcon from "@mui/icons-material/CloudUploadOutlined";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { useMatch, useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import type { EntryHeader } from "../types";
import { formatDayNumber, formatTime, formatWeekday } from "../utils/date";
import { TRACKING, TYPE, rem } from "./chrome";

/// A row's sync mark: the exception, not decoration. A synced row shows
/// nothing, and neither does any row while sync is off, or every row would
/// carry one. A conflict outranks "not uploaded", since a parked conflict is
/// also waiting to be pushed.
///
/// The icon is sized like the time beside it, so it sits inside that line's
/// own height and a marked row is no taller than an unmarked one — and it
/// stays that way at any text size, being in the same unit. The label is on
/// the wrapper, which is what `role="img"` and the tooltip both name.
function SyncMark({ id }: { id: string }) {
  const { sync, pendingIds, conflictedIds } = useAppState();
  if (sync.state === "off") return null;
  const conflicted = conflictedIds.has(id);
  if (!conflicted && !pendingIds.has(id)) return null;

  const label = conflicted
    ? "Conflicting versions — resolving"
    : "Not uploaded yet";
  return (
    <Tooltip title={label} placement="top" arrow>
      <Box
        component="span"
        role="img"
        aria-label={label}
        data-testid="sync-mark"
        sx={{
          display: "inline-flex",
          color: conflicted ? "warning.main" : "inherit",
          opacity: conflicted ? 1 : 0.6,
        }}
      >
        {conflicted ? (
          <WarningAmberIcon sx={{ fontSize: TYPE.row }} />
        ) : (
          <CloudUploadOutlinedIcon sx={{ fontSize: TYPE.row }} />
        )}
      </Box>
    </Tooltip>
  );
}

/// One row, in the journal or in a result list — the same row either way, so
/// that a result is recognisably the entry it will open.
///
/// `snippet` is the only difference: a search result carries a line of the
/// entry's text, which is the one place in this column body text appears. The
/// journal's own rows have none (see the component below for why).
export default function EntryRow({
  entry,
  snippet,
}: {
  entry: EntryHeader;
  snippet?: string;
}) {
  const navigate = useNavigate();
  // `useParams` would be empty here: this column is rendered by the layout
  // route, which has no path of its own, so the child's `:id` never reaches it.
  const selectedId = useMatch("/entries/:id")?.params.id;

  return (
    <ListItemButton
      data-testid="entry-row"
      selected={entry.id === selectedId}
      onClick={() => navigate(`/entries/${entry.id}`)}
      sx={{
        alignItems: "flex-start",
        gap: 1.5,
        px: 2,
        py: 1.25,
        borderRadius: 0,
        borderBottom: 1,
        borderColor: "divider",
        "&.Mui-selected": {
          bgcolor: "tonal.primaryContainer",
          color: "tonal.onPrimaryContainer",
          "&:hover": { bgcolor: "tonal.primaryContainer" },
        },
      }}
    >
      {/* Wide enough for a two-digit day at `lead`, in the same unit, so it
          stays wide enough when the reader's text size goes up. */}
      <Box sx={{ width: rem(34), flexShrink: 0, textAlign: "center" }}>
        <Typography
          sx={{ fontSize: TYPE.lead, fontWeight: 700, lineHeight: 1.15 }}
        >
          {formatDayNumber(entry.entry_date)}
        </Typography>
        <Typography
          sx={{
            fontSize: TYPE.micro,
            letterSpacing: TRACKING.wide,
            textTransform: "uppercase",
            opacity: 0.7,
          }}
        >
          {formatWeekday(entry.entry_date)}
        </Typography>
      </Box>
      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Typography noWrap sx={{ fontSize: TYPE.row, fontWeight: 600 }}>
          {entry.title}
        </Typography>
        <Stack direction="row" alignItems="center" spacing={0.75}>
          <Typography sx={{ fontSize: TYPE.meta, opacity: 0.7 }}>
            {formatTime(entry.entry_date)}
          </Typography>
          <SyncMark id={entry.id} />
        </Stack>
        {snippet && (
          <Typography
            data-testid="search-snippet"
            sx={{
              fontSize: TYPE.meta,
              opacity: 0.8,
              mt: 0.25,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {snippet}
          </Typography>
        )}
      </Box>
    </ListItemButton>
  );
}
