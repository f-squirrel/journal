import { useEffect, useRef } from "react";
import {
  Alert,
  Box,
  Button,
  Fab,
  IconButton,
  InputBase,
  List,
  ListSubheader,
  Stack,
  Typography,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import ClearIcon from "@mui/icons-material/Clear";
import MenuIcon from "@mui/icons-material/Menu";
import NotesIcon from "@mui/icons-material/Notes";
import SearchIcon from "@mui/icons-material/Search";
import { useAppState } from "../context/AppStateContext";
import { useNewEntry } from "../hooks/useNewEntry";
import { groupByMonth } from "../utils/date";
import { HEADER_HEIGHT, LIST_WIDTH } from "./chrome";
import EntryRow from "./EntryRow";

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
  const { entries, searchQuery, searchResults, searchError, listError } =
    useAppState();
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
        {/* Likewise a failed reload: the rows already loaded stay below it. */}
        {listError && (
          <Alert severity="error" square sx={{ fontSize: 13 }}>
            {listError}
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
        ) : entries.length > 0 ? (
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
        ) : listError ? null : (
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
