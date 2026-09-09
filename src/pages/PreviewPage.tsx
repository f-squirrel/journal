import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Paper,
  Stack,
  Typography,
} from "@mui/material";
import ArrowBackIcon from "@mui/icons-material/ArrowBack";
import DeleteIcon from "@mui/icons-material/Delete";
import EditIcon from "@mui/icons-material/Edit";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import AppShell from "../components/AppShell";
import { useAppState } from "../context/AppStateContext";
import type { JournalEntry } from "../types";
import { formatUpdatedAt } from "../utils/date";
import { errorMessage } from "../utils/errors";

export default function PreviewPage() {
  const { id } = useParams<{ id: string }>();
  const { loadEntry, removeEntry } = useAppState();
  const navigate = useNavigate();
  const [entry, setEntry] = useState<JournalEntry | undefined>(undefined);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState("");

  useEffect(() => {
    if (!id) {
      setEntry(undefined);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setLoadError("");
    loadEntry(id)
      .then((loaded) => {
        if (cancelled) return;
        setEntry(loaded);
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
  }, [id, loadEntry]);

  if (loading) {
    return null;
  }

  if (loadError) {
    return (
      <AppShell>
        <Alert severity="error" sx={{ maxWidth: 720, mx: "auto", mt: 6 }}>
          {loadError}
        </Alert>
      </AppShell>
    );
  }

  if (!entry) {
    return (
      <AppShell>
        <Typography color="text.secondary" textAlign="center" sx={{ mt: 6 }}>
          Entry not found.
        </Typography>
      </AppShell>
    );
  }

  async function handleDelete() {
    if (!entry) return;
    setDeleting(true);
    setDeleteError("");
    try {
      await removeEntry(entry.id);
    } catch (err) {
      // The file is still on disk, so keep the dialog open and usable rather
      // than navigating back to a list the entry has vanished from.
      setDeleteError(errorMessage(err));
      return;
    } finally {
      setDeleting(false);
    }
    navigate("/entries");
  }

  return (
    <AppShell>
      <Box sx={{ maxWidth: 720, mx: "auto" }}>
        <Stack direction="row" justifyContent="space-between" sx={{ mb: 3 }}>
          <Button startIcon={<ArrowBackIcon />} onClick={() => navigate("/entries")}>
            Back
          </Button>
          <Stack direction="row" spacing={1.5}>
            <Button
              color="error"
              startIcon={<DeleteIcon />}
              onClick={() => {
                setDeleteError("");
                setConfirmOpen(true);
              }}
            >
              Delete
            </Button>
            <Button
              variant="contained"
              disableElevation
              startIcon={<EditIcon />}
              onClick={() => navigate(`/entries/${entry.id}/edit`)}
            >
              Edit
            </Button>
          </Stack>
        </Stack>
        <Typography variant="h4" fontWeight={700}>
          {entry.title}
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
          Updated {formatUpdatedAt(entry.updated_at)}
        </Typography>
        <Paper
          elevation={0}
          sx={{ p: { xs: 2.5, sm: 4 }, bgcolor: "tonal.surfaceContainerLow" }}
          className="markdown-body"
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{entry.markdown}</ReactMarkdown>
        </Paper>
      </Box>

      <Dialog open={confirmOpen} onClose={() => setConfirmOpen(false)}>
        <DialogTitle>Delete entry?</DialogTitle>
        <DialogContent>
          <DialogContentText>
            {`"${entry.title}" will be permanently deleted. This can't be undone.`}
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
    </AppShell>
  );
}
