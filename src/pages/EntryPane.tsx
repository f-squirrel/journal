import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";
import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  IconButton,
  InputBase,
  Menu,
  MenuItem,
  Popover,
  Stack,
  Typography,
  useMediaQuery,
  useTheme,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import ArrowBackIcon from "@mui/icons-material/ArrowBack";
import AutoStoriesIcon from "@mui/icons-material/AutoStories";
import DeleteIcon from "@mui/icons-material/Delete";
import EditIcon from "@mui/icons-material/Edit";
import MoreHorizIcon from "@mui/icons-material/MoreHoriz";
import VisibilityIcon from "@mui/icons-material/Visibility";
import { DateCalendar } from "@mui/x-date-pickers/DateCalendar";
import { openUrl } from "@tauri-apps/plugin-opener";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { backend } from "../api/backend";
import {
  BODY_MIN_HEIGHT,
  HEADER_HEIGHT,
  READING_WIDTH,
  TRACKING,
  TYPE,
  rem,
} from "../components/chrome";
import MarkdownEditor from "../components/MarkdownEditor";
import { useAppState } from "../context/AppStateContext";
import {
  useAutosave,
  type Autosave,
  type SaveStatus,
} from "../hooks/useAutosave";
import { useNewEntry } from "../hooks/useNewEntry";
import { formatAgo, formatEntryHeading, formatTime } from "../utils/date";
import { errorMessage } from "../utils/errors";

const STATUS_TEXT: Record<SaveStatus, string> = {
  idle: "",
  saving: "Saving…",
  saved: "Saved",
  error: "Couldn't save",
};

/// The right-hand pane with nothing open in it.
export function NoEntrySelected() {
  const newEntry = useNewEntry();

  return (
    <Stack
      alignItems="center"
      justifyContent="center"
      spacing={2}
      sx={{ height: "100%", px: 4 }}
    >
      <AutoStoriesIcon sx={{ fontSize: rem(44), color: "text.disabled" }} />
      <Typography color="text.secondary">
        Select an entry, or start a new one.
      </Typography>
      <Button startIcon={<AddIcon />} onClick={newEntry}>
        New entry
      </Button>
    </Stack>
  );
}

/// The route element. It keys the pane on the entry id so that opening another
/// entry mounts a fresh one — the draft state resets for free, and the old
/// pane's unmount flushes whatever was still unsaved.
export default function EntryPaneRoute() {
  const { id } = useParams<{ id: string }>();
  const location = useLocation();
  // Set by `useNewEntry`: this id is not on disk yet, so there is nothing to
  // load and "Entry not found." would be wrong.
  const draft = (location.state as { draft?: boolean } | null)?.draft === true;

  if (!id) return <NoEntrySelected />;
  return <EntryPane key={id} id={id} draft={draft} />;
}

function EntryPane({ id, draft }: { id: string; draft: boolean }) {
  const { loadEntry, removeEntry, syncVersion } = useAppState();
  const navigate = useNavigate();
  const theme = useTheme();
  const compact = useMediaQuery(theme.breakpoints.down("md"));

  const [loading, setLoading] = useState(!draft);
  const [loadError, setLoadError] = useState("");
  const [missing, setMissing] = useState(false);
  const [loadedAt, setLoadedAt] = useState("");
  const [title, setTitle] = useState("");
  const [markdown, setMarkdown] = useState("");
  // Reading or writing, chosen from the menu and nowhere else. A draft has
  // nothing to render, so it opens in the editor.
  const [mode, setMode] = useState<"read" | "edit">(draft ? "edit" : "read");
  const [menuAnchor, setMenuAnchor] = useState<HTMLElement | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState("");
  // Dismissible: a fresh notice un-dismisses itself, since it describes a new
  // event, not the one the user already closed.
  const [noticeDismissed, setNoticeDismissed] = useState(false);
  // A new entry is about today until the user says otherwise. Computed once,
  // so it is stable for the life of the pane.
  const [openedAt] = useState(() => new Date().toISOString());
  // The entry's own date: what the heading shows, what the list sorts by, and
  // the one field here the user edits without typing.
  const [entryDate, setEntryDate] = useState(openedAt);
  const [dateAnchor, setDateAnchor] = useState<HTMLElement | null>(null);

  // Reaches the autosave hook through a ref rather than a closure variable:
  // `refreshFromDisk` is itself passed into `useAutosave` as `onStale`, so
  // `autosave` cannot exist yet at the point `refreshFromDisk` is declared.
  const autosaveRef = useRef<Autosave | null>(null);

  /// `MarkdownEditor` reports every change to the CodeMirror document through
  /// `onChange`, including the one *it* makes when `value` changes out from
  /// under it (its own effect diffs the doc against the new prop and dispatches
  /// the difference) — there is no separate "this came from outside" signal
  /// from CodeMirror's update listener. Set to the text `refreshFromDisk` just
  /// wrote whenever it touches `markdown` while in edit mode, so `changeMarkdown`
  /// can tell that echo apart from an actual keystroke and skip `noteChange`
  /// for it — otherwise a reload while the user is looking at the editor both
  /// erases the notice (`noteChange` clears it) and queues a pointless resave.
  const externalMarkdown = useRef<string | null>(null);

  /// Reloads the entry from disk and adopts it, but only if this pane has
  /// nothing unsaved — a dirty draft is left alone, and its next save meets
  /// Stale and becomes a conflicted copy instead (see `useAutosave`). Checked
  /// again after the load resolves, since typing can start while it is in
  /// flight. Used both by the `syncVersion` effect below, for a pass that
  /// changed entries on disk, and as `onStale`, once a stale save of this
  /// pane's own draft has already been kept as a copy elsewhere. Never sets
  /// `loading`, so the editor stays mounted and keeps its undo history.
  const refreshFromDisk = useCallback(() => {
    const autosave = autosaveRef.current;
    if (!autosave || autosave.hasUnsaved()) return;
    loadEntry(id).then((loaded) => {
      const current = autosaveRef.current;
      if (!current || current.hasUnsaved()) return;
      if (!loaded) {
        // A brand-new draft that has never reached the vault is not "missing"
        // — there is nothing on disk yet for it to have gone missing from.
        if (draft && current.savedAt === null) return;
        setMissing(true);
        return;
      }
      setTitle(loaded.title);
      externalMarkdown.current = loaded.markdown;
      setMarkdown(loaded.markdown);
      setEntryDate(loaded.entry_date);
      setLoadedAt(loaded.updated_at);
      current.setBase(loaded.updated_at);
    });
  }, [draft, id, loadEntry]);

  const autosave = useAutosave(
    id,
    { title: "", markdown: "", entryDate },
    draft,
    refreshFromDisk,
  );

  useEffect(() => {
    autosaveRef.current = autosave;
  });

  // A new notice starts undismissed, even if the last one was closed.
  useEffect(() => {
    setNoticeDismissed(false);
  }, [autosave.notice]);

  // A pane with nothing unsaved refreshes itself when a sync pass changed
  // entries on disk (`sync://changed`, via `syncVersion`). Skips the run that
  // fires right after mount: there is nothing to refresh from yet, since the
  // load effect below has either not started or just seeded the same state.
  const skipNextSyncVersion = useRef(true);
  useEffect(() => {
    if (skipNextSyncVersion.current) {
      skipNextSyncVersion.current = false;
      return;
    }
    refreshFromDisk();
  }, [syncVersion, refreshFromDisk]);

  useEffect(() => {
    if (draft) return;
    let cancelled = false;
    setLoading(true);
    setLoadError("");
    setMissing(false);
    loadEntry(id)
      .then((loaded) => {
        if (cancelled) return;
        if (!loaded) {
          setMissing(true);
        } else {
          setTitle(loaded.title);
          setMarkdown(loaded.markdown);
          setLoadedAt(loaded.updated_at);
          setEntryDate(loaded.entry_date);
          autosave.setBase(loaded.updated_at);
        }
        setLoading(false);
      })
      .catch((err) => {
        // A failed read is not a missing entry — say which one it was, rather
        // than rendering "Entry not found." over a vault that is merely locked.
        if (cancelled) return;
        setLoadError(errorMessage(err));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [draft, id, loadEntry]);

  if (loading) {
    return null;
  }

  if (loadError) {
    return (
      <Box sx={{ p: 4 }}>
        <Alert
          severity="error"
          sx={{ maxWidth: READING_WIDTH, mx: "auto", mt: 4 }}
        >
          {loadError}
        </Alert>
      </Box>
    );
  }

  if (missing) {
    return (
      <Typography color="text.secondary" textAlign="center" sx={{ mt: 8 }}>
        Entry not found.
      </Typography>
    );
  }

  function changeTitle(next: string) {
    setTitle(next);
    autosave.noteChange({ title: next, markdown, entryDate });
  }

  function changeMarkdown(next: string) {
    setMarkdown(next);
    // The editor reporting back the text a reload just gave it, not a
    // keystroke — see `externalMarkdown`. Consumed once, so a real edit
    // straight after (even one that happens to retype the same text) is not
    // swallowed too.
    if (externalMarkdown.current !== null) {
      const wasExternal = next === externalMarkdown.current;
      externalMarkdown.current = null;
      if (wasExternal) return;
    }
    autosave.noteChange({ title, markdown: next, entryDate });
  }

  /// Takes the day from the picker and keeps the time of day the entry
  /// already had, so re-dating an entry does not also move it to midnight.
  function changeEntryDate(day: Date | null) {
    setDateAnchor(null);
    if (!day || Number.isNaN(day.getTime())) return;
    const next = new Date(entryDate);
    next.setFullYear(day.getFullYear(), day.getMonth(), day.getDate());
    const iso = next.toISOString();
    setEntryDate(iso);
    // The same path a title edit takes: an edit is an edit.
    autosave.noteChange({ title, markdown, entryDate: iso });
  }

  function startEditing() {
    setMenuAnchor(null);
    setMode("edit");
  }

  function stopEditing() {
    setMenuAnchor(null);
    // Write before rendering: the read view must never show text that the
    // vault has not been handed yet, and the status should say so.
    autosave.saveNow();
    setMode("read");
  }

  async function handleDelete() {
    setDeleting(true);
    setDeleteError("");
    // Before the request, not after: a save scheduled by the last keystroke
    // would otherwise land on a deleted entry and write it straight back.
    let hadPendingSave = autosave.cancel();
    // A write already on its way has to land before the delete, or it lands
    // after it and puts the entry back.
    await autosave.whenSettled();
    // A write that failed while we waited has marked the draft dirty again —
    // the pane's unmount flush would otherwise write the entry straight back
    // after the delete below has succeeded.
    hadPendingSave = autosave.cancel() || hadPendingSave;
    try {
      await removeEntry(id);
    } catch (err) {
      // The entry is still on disk, so keep the dialog open and usable — and
      // re-arm the save that was cancelled on the way in.
      setDeleteError(errorMessage(err));
      if (hadPendingSave) autosave.noteChange({ title, markdown, entryDate });
      return;
    } finally {
      setDeleting(false);
    }
    navigate("/entries");
  }

  // `updated_at`, which the heading no longer shows: the heading is the
  // entry's date now, and when it was last written is a different fact. Kept
  // as a tooltip on the save status rather than dropped.
  const writtenAt = autosave.savedAt ?? loadedAt;

  return (
    <Stack sx={{ height: "100%", minHeight: 0 }}>
      <Stack
        direction="row"
        alignItems="center"
        spacing={1}
        sx={{
          px: 1.5,
          height: HEADER_HEIGHT,
          borderBottom: 1,
          borderColor: "divider",
          flexShrink: 0,
        }}
      >
        {compact && (
          <IconButton aria-label="Back" onClick={() => navigate("/entries")}>
            <ArrowBackIcon />
          </IconButton>
        )}
        <Box sx={{ flex: 1 }} />
        <Typography
          data-testid="save-status"
          variant="caption"
          title={
            writtenAt ? `Last edited ${formatAgo(writtenAt, Date.now())}` : ""
          }
          color={autosave.status === "error" ? "error" : "text.secondary"}
        >
          {STATUS_TEXT[autosave.status]}
        </Typography>
        <IconButton
          aria-label="Entry actions"
          onClick={(event) => setMenuAnchor(event.currentTarget)}
        >
          <MoreHorizIcon />
        </IconButton>
      </Stack>

      {autosave.status === "error" && (
        <Alert
          severity="error"
          sx={{ borderRadius: 0 }}
          action={
            <Button color="inherit" size="small" onClick={autosave.saveNow}>
              Retry
            </Button>
          }
        >
          {autosave.error}
        </Alert>
      )}

      {autosave.notice && !noticeDismissed && (
        <Alert
          severity="info"
          sx={{ borderRadius: 0 }}
          onClose={() => setNoticeDismissed(true)}
        >
          {autosave.notice}
        </Alert>
      )}

      <Box sx={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
        <Box
          sx={{
            maxWidth: READING_WIDTH,
            mx: "auto",
            px: { xs: 2.5, sm: 5 },
            py: { xs: 3, sm: 5 },
          }}
        >
          {/* The heading is the entry's date, so it is also the control that
              changes it — there is nowhere else in the pane it would belong. */}
          <Button
            data-testid="entry-heading"
            aria-label={`Change entry date (${formatEntryHeading(entryDate)})`}
            onClick={(event) => setDateAnchor(event.currentTarget)}
            sx={{
              p: 0,
              minWidth: 0,
              textAlign: "left",
              display: "block",
              color: "text.primary",
              typography: "h5",
              textTransform: "none",
              "&:hover": {
                bgcolor: "transparent",
                textDecoration: "underline",
              },
            }}
          >
            {formatEntryHeading(entryDate)}
          </Button>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
            {formatTime(entryDate)}
          </Typography>

          <Popover
            open={dateAnchor !== null}
            anchorEl={dateAnchor}
            onClose={() => setDateAnchor(null)}
            anchorOrigin={{ vertical: "bottom", horizontal: "left" }}
          >
            {/* No slide transition between months: it is noise in a popover
                this small, and it keeps the month being left behind in the
                DOM, where it shadows the one actually on screen. */}
            <DateCalendar
              value={new Date(entryDate)}
              onChange={changeEntryDate}
              reduceAnimations
            />
          </Popover>

          {mode === "edit" ? (
            <>
              <InputBase
                value={title}
                onChange={(event) => changeTitle(event.target.value)}
                placeholder="Title"
                fullWidth
                sx={{
                  mb: 1.5,
                  fontSize: TYPE.entryTitle,
                  fontWeight: 700,
                  letterSpacing: TRACKING.tight,
                  "& input": { p: 0 },
                }}
              />
              <MarkdownEditor
                value={markdown}
                onChange={changeMarkdown}
                placeholder="Write your entry..."
                autoFocus
              />
            </>
          ) : (
            <>
              <Typography
                variant="h5"
                component="h2"
                sx={{ mb: 1.5, fontSize: TYPE.entryTitle }}
              >
                {title || "Untitled entry"}
              </Typography>
              {/* Same test id as the editor: the body is the body, whichever
                  mode is showing it. */}
              <Box
                data-testid="entry-body"
                className="markdown-body"
                sx={{
                  minHeight: BODY_MIN_HEIGHT,
                  // The two rules that need the palette, so they cannot live
                  // in `index.css` with the rest of the markdown styling.
                  "& a": { color: "primary.main" },
                  "& img": { maxWidth: "100%", borderRadius: 1 },
                }}
              >
                <ReactMarkdown
                  remarkPlugins={[remarkGfm]}
                  components={{
                    // The window has no address bar, so a click must never
                    // navigate it: an entry's links open in the user's
                    // browser, where they can be seen.
                    a: ({ href, children }) => (
                      <a
                        href={href}
                        onClick={(event) => {
                          event.preventDefault();
                          if (!href) return;
                          if (backend) void openUrl(href);
                          else
                            window.open(href, "_blank", "noopener,noreferrer");
                        }}
                      >
                        {children}
                      </a>
                    ),
                  }}
                >
                  {markdown}
                </ReactMarkdown>
              </Box>
            </>
          )}
        </Box>
      </Box>

      <Menu
        anchorEl={menuAnchor}
        open={menuAnchor !== null}
        onClose={() => setMenuAnchor(null)}
      >
        {mode === "read" ? (
          <MenuItem onClick={startEditing}>
            <EditIcon fontSize="small" sx={{ mr: 1.5 }} />
            Edit
          </MenuItem>
        ) : (
          <MenuItem onClick={stopEditing}>
            <VisibilityIcon fontSize="small" sx={{ mr: 1.5 }} />
            Preview
          </MenuItem>
        )}
        <MenuItem
          onClick={() => {
            setMenuAnchor(null);
            setDeleteError("");
            setConfirmOpen(true);
          }}
        >
          <DeleteIcon fontSize="small" sx={{ mr: 1.5 }} />
          Delete entry
        </MenuItem>
      </Menu>

      <Dialog open={confirmOpen} onClose={() => setConfirmOpen(false)}>
        <DialogTitle>Delete entry?</DialogTitle>
        <DialogContent>
          <DialogContentText>
            {`"${title.trim() || "Untitled entry"}" will be permanently deleted. This can't be undone.`}
          </DialogContentText>
          {deleteError && (
            <Alert severity="error" sx={{ mt: 2 }}>
              {deleteError}
            </Alert>
          )}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirmOpen(false)} disabled={deleting}>
            Cancel
          </Button>
          <Button color="error" onClick={handleDelete} disabled={deleting}>
            Delete
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}
