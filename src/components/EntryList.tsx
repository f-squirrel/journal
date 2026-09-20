import { useEffect, useRef } from "react";
import {
  Alert,
  Box,
  Button,
  Fab,
  IconButton,
  InputBase,
  List,
  ListItemButton,
  ListSubheader,
  Stack,
  Tooltip,
  Typography,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import ClearIcon from "@mui/icons-material/Clear";
import MenuIcon from "@mui/icons-material/Menu";
import NotesIcon from "@mui/icons-material/Notes";
import SearchIcon from "@mui/icons-material/Search";
import CloudUploadOutlinedIcon from "@mui/icons-material/CloudUploadOutlined";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { useMatch, useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";
import { useNewEntry } from "../hooks/useNewEntry";
import type { EntryHeader } from "../types";
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

/// One row, in the journal or in a result list — the same row either way, so
/// that a result is recognisably the entry it will open.
///
/// `snippet` is the only difference: a search result carries a line of the
/// entry's text, which is the one place in this column body text appears. The
/// journal's own rows have none (see the component below for why).
function EntryRow({
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
      <Box sx={{ width: 34, flexShrink: 0, textAlign: "center" }}>
        <Typography sx={{ fontSize: 20, fontWeight: 700, lineHeight: 1.15 }}>
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
        {snippet && (
          <Typography
            data-testid="search-snippet"
            sx={{
              fontSize: 12,
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

/// The search field, above the journal and below the column header.
///
/// Ctrl/Cmd-F focuses it from anywhere in the app — including from inside the
/// editor, which is where the user will be when they want it — and Escape
/// clears it, then gives the keyboard back.
function SearchField() {
  const { searchQuery, setSearchQuery, searching } = useAppState();
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (!(event.ctrlKey || event.metaKey) || event.key !== "f") return;
      // The browser's own find bar would search the rendered DOM, which holds
      // one entry and a list of titles — not the journal.
      event.preventDefault();
      input.current?.focus();
      input.current?.select();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <Box
      sx={{
        px: 1.5,
        py: 1,
        borderBottom: 1,
        borderColor: "divider",
        flexShrink: 0,
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={0.5}
        sx={{
          px: 1,
          borderRadius: 2,
          bgcolor: "tonal.surfaceContainerHigh",
          height: 34,
        }}
      >
        <SearchIcon sx={{ fontSize: 18, opacity: searching ? 1 : 0.6 }} />
        <InputBase
          inputRef={input}
          value={searchQuery}
          onChange={(event) => setSearchQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            if (searchQuery) {
              setSearchQuery("");
            } else {
              input.current?.blur();
            }
          }}
          placeholder="Search entries"
          inputProps={{ "aria-label": "Search entries" }}
          sx={{ flex: 1, fontSize: 13 }}
        />
        {searchQuery && (
          <IconButton
            size="small"
            aria-label="Clear search"
            onClick={() => {
              setSearchQuery("");
              input.current?.focus();
            }}
          >
            <ClearIcon sx={{ fontSize: 16 }} />
          </IconButton>
        )}
      </Stack>
    </Box>
  );
}

/// The middle column: every entry, newest first, under sticky month headers —
/// or, while a search is running, what it matched.
///
/// Journal rows carry a title and a time but no excerpt: `list_entries`
/// returns headers without `markdown` on purpose (see `crates/vault`), and
/// filling excerpts would mean decrypting the whole journal just to draw the
/// list. A search result does carry a line of text, and that is a different
/// trade, not a reversal of this one: it decrypts on demand, for someone who
/// asked a question, rather than on every render of every session.
export default function EntryList({
  onOpenSidebar,
}: {
  onOpenSidebar?: () => void;
}) {
  const { entries, searchQuery, searchResults, searchError } = useAppState();
  const newEntry = useNewEntry();

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
          {(searchResults ?? entries).length}
        </Typography>
      </Stack>

      <SearchField />

      <Box sx={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
        {/* A failed search leaves `searchResults` null, so the journal is
            still below this and the user has not lost their place. */}
        {searchError && (
          <Alert severity="error" square sx={{ fontSize: 13 }}>
            {searchError}
          </Alert>
        )}

        {searchResults !== null ? (
          searchResults.length === 0 ? (
            <Stack
              alignItems="center"
              spacing={1}
              sx={{ px: 3, py: 6, textAlign: "center" }}
            >
              <SearchIcon sx={{ fontSize: 30, color: "text.disabled" }} />
              <Typography variant="body2" color="text.secondary">
                Nothing matches “{searchQuery.trim()}”.
              </Typography>
            </Stack>
          ) : (
            <List disablePadding>
              {searchResults.map((match) => (
                <EntryRow
                  key={match.header.id}
                  entry={match.header}
                  snippet={match.snippet}
                />
              ))}
            </List>
          )
        ) : entries.length === 0 ? (
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
                <EntryRow key={entry.id} entry={entry} />
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
