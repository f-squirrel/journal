import {
  Box,
  Button,
  Fab,
  IconButton,
  List,
  ListItemButton,
  ListSubheader,
  Stack,
  Tooltip,
  Typography,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import MenuIcon from "@mui/icons-material/Menu";
import NotesIcon from "@mui/icons-material/Notes";
import CloudUploadOutlinedIcon from "@mui/icons-material/CloudUploadOutlined";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { useMatch, useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import { useNewEntry } from "../hooks/useNewEntry";
import {
  formatDayNumber,
  formatTime,
  formatWeekday,
  groupByMonth,
} from "../utils/date";
import { HEADER_HEIGHT, LIST_WIDTH } from "./chrome";

/// A row's sync mark: the exception, not decoration. A synced row shows
/// nothing, and neither does any row while sync is off, or every row would
/// carry one. A conflict outranks "not uploaded", since a parked conflict is
/// also waiting to be pushed.
///
/// The icon is 14px inside the time line's own height, so a marked row is no
/// taller than an unmarked one. The label is on the wrapper, which is what
/// `role="img"` and the tooltip both name.
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
          <WarningAmberIcon sx={{ fontSize: 14 }} />
        ) : (
          <CloudUploadOutlinedIcon sx={{ fontSize: 14 }} />
        )}
      </Box>
    </Tooltip>
  );
}

/// The middle column: every entry, newest first, under sticky month headers.
///
/// Rows carry a title and a time but no excerpt: `list_entries` returns
/// headers without `markdown` on purpose (see `crates/vault`), and filling
/// excerpts would mean decrypting the whole journal just to draw the list.
export default function EntryList({
  onOpenSidebar,
}: {
  onOpenSidebar?: () => void;
}) {
  const { entries } = useAppState();
  const navigate = useNavigate();
  const newEntry = useNewEntry();
  // `useParams` would be empty here: this column is rendered by the layout
  // route, which has no path of its own, so the child's `:id` never reaches it.
  const selectedId = useMatch("/entries/:id")?.params.id;

  const months = groupByMonth(entries);

  return (
    <Stack
      sx={{
        width: { xs: "100%", md: LIST_WIDTH },
        flexShrink: 0,
        height: "100%",
        minHeight: 0,
        position: "relative",
        bgcolor: "tonal.surfaceContainerLow",
        borderRight: { md: 1 },
        borderColor: { md: "divider" },
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={1}
        sx={{
          px: 2,
          height: HEADER_HEIGHT,
          borderBottom: 1,
          borderColor: "divider",
          flexShrink: 0,
        }}
      >
        {onOpenSidebar && (
          <IconButton
            edge="start"
            onClick={onOpenSidebar}
            aria-label="Open menu"
            size="small"
          >
            <MenuIcon />
          </IconButton>
        )}
        <Typography variant="h6" sx={{ flex: 1, fontSize: 17 }}>
          All entries
        </Typography>
        <Typography variant="caption" color="text.secondary">
          {entries.length}
        </Typography>
      </Stack>

      <Box sx={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
        {entries.length === 0 ? (
          <Stack
            alignItems="center"
            spacing={2}
            sx={{ px: 3, py: 8, textAlign: "center" }}
          >
            <NotesIcon sx={{ fontSize: 36, color: "text.disabled" }} />
            <Typography variant="body2" color="text.secondary">
              No entries yet. Start your journal.
            </Typography>
            <Button
              variant="contained"
              disableElevation
              startIcon={<AddIcon />}
              onClick={newEntry}
            >
              Write your first entry
            </Button>
          </Stack>
        ) : (
          <List disablePadding>
            {months.map((month) => [
              <ListSubheader
                key={month.key}
                sx={{
                  bgcolor: "tonal.surfaceContainerLow",
                  color: "text.secondary",
                  fontSize: 11,
                  fontWeight: 700,
                  letterSpacing: 0.8,
                  textTransform: "uppercase",
                  lineHeight: 2.6,
                }}
              >
                {month.label}
              </ListSubheader>,
              ...month.entries.map((entry) => (
                <ListItemButton
                  key={entry.id}
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
                  <Box sx={{ width: 34, flexShrink: 0, textAlign: "center" }}>
                    <Typography
                      sx={{ fontSize: 20, fontWeight: 700, lineHeight: 1.15 }}
                    >
                      {formatDayNumber(entry.entry_date)}
                    </Typography>
                    <Typography
                      sx={{
                        fontSize: 10,
                        letterSpacing: 0.6,
                        textTransform: "uppercase",
                        opacity: 0.7,
                      }}
                    >
                      {formatWeekday(entry.entry_date)}
                    </Typography>
                  </Box>
                  <Box sx={{ minWidth: 0, flex: 1 }}>
                    <Typography noWrap sx={{ fontSize: 14, fontWeight: 600 }}>
                      {entry.title}
                    </Typography>
                    <Stack direction="row" alignItems="center" spacing={0.75}>
                      <Typography sx={{ fontSize: 12, opacity: 0.7 }}>
                        {formatTime(entry.entry_date)}
                      </Typography>
                      <SyncMark id={entry.id} />
                    </Stack>
                  </Box>
                </ListItemButton>
              )),
            ])}
          </List>
        )}
      </Box>

      {/* Compact layout only: the sidebar's "New entry" button is behind the
          drawer there, and starting an entry is the one thing that must never
          be two taps away. */}
      {onOpenSidebar && (
        <Fab
          aria-label="New entry"
          onClick={newEntry}
          sx={{ position: "absolute", right: 20, bottom: 20 }}
        >
          <AddIcon />
        </Fab>
      )}
    </Stack>
  );
}
