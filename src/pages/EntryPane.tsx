import { useEffect, useState } from "react";
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
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { HEADER_HEIGHT } from "../components/chrome";
import MarkdownEditor from "../components/MarkdownEditor";
import { useAppState } from "../context/AppStateContext";
import { useAutosave, type SaveStatus } from "../hooks/useAutosave";
import { useNewEntry } from "../hooks/useNewEntry";
import { formatEntryHeading, formatTime } from "../utils/date";
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
    <Stack alignItems="center" justifyContent="center" spacing={2} sx={{ height: "100%", px: 4 }}>
      <AutoStoriesIcon sx={{ fontSize: 44, color: "text.disabled" }} />
      <Typography color="text.secondary">Select an entry, or start a new one.</Typography>
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
  const { loadEntry, removeEntry } = useAppState();
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
  // Stands in for `updated_at` until the draft's first save, so the date
  // heading reads as today rather than as nothing.
  const [openedAt] = useState(() => new Date().toISOString());

  const autosave = useAutosave(id);

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
        <Alert severity="error" sx={{ maxWidth: 720, mx: "auto", mt: 4 }}>
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
    autosave.noteChange({ title: next, markdown });
  }

  function changeMarkdown(next: string) {
    setMarkdown(next);
    autosave.noteChange({ title, markdown: next });
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
    const hadPendingSave = autosave.cancel();
    try {
      await removeEntry(id);
    } catch (err) {
      // The entry is still on disk, so keep the dialog open and usable — and
      // re-arm the save that was cancelled on the way in.
      setDeleteError(errorMessage(err));
      if (hadPendingSave) autosave.noteChange({ title, markdown });
      return;
    } finally {
      setDeleting(false);
    }
    navigate("/entries");
  }

  const timestamp = autosave.savedAt ?? (loadedAt || openedAt);

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
          color={autosave.status === "error" ? "error" : "text.secondary"}
        >
          {STATUS_TEXT[autosave.status]}
        </Typography>
        <IconButton aria-label="Entry actions" onClick={(event) => setMenuAnchor(event.currentTarget)}>
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

      <Box sx={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
        <Box sx={{ maxWidth: 720, mx: "auto", px: { xs: 2.5, sm: 5 }, py: { xs: 3, sm: 5 } }}>
          <Typography variant="h5" component="h1">
            {formatEntryHeading(timestamp)}
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
            {formatTime(timestamp)}
          </Typography>

          {mode === "edit" ? (
            <>
              <InputBase
                value={title}
                onChange={(event) => changeTitle(event.target.value)}
                placeholder="Title"
                fullWidth
                sx={{
                  mb: 1.5,
                  fontSize: "1.45rem",
                  fontWeight: 700,
                  letterSpacing: "-0.3px",
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
              <Typography variant="h5" component="h2" sx={{ mb: 1.5, fontSize: "1.45rem" }}>
                {title || "Untitled entry"}
              </Typography>
              {/* Same test id as the editor: the body is the body, whichever
                  mode is showing it. */}
              <Box
                data-testid="entry-body"
                className="markdown-body"
                sx={{
                  minHeight: 260,
                  // The two rules that need the palette, so they cannot live
                  // in `index.css` with the rest of the markdown styling.
                  "& a": { color: "primary.main" },
                  "& img": { maxWidth: "100%", borderRadius: 1 },
                }}
              >
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{markdown}</ReactMarkdown>
              </Box>
            </>
          )}
        </Box>
      </Box>

      <Menu anchorEl={menuAnchor} open={menuAnchor !== null} onClose={() => setMenuAnchor(null)}>
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
