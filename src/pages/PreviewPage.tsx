import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
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

export default function PreviewPage() {
  const { id } = useParams<{ id: string }>();
  const { loadEntry, removeEntry } = useAppState();
  const navigate = useNavigate();
  const [entry, setEntry] = useState<JournalEntry | undefined>(undefined);
  const [loading, setLoading] = useState(true);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    if (!id) {
      setEntry(undefined);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    loadEntry(id).then((loaded) => {
      if (cancelled) return;
      setEntry(loaded);
      setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [id, loadEntry]);

  if (loading) {
    return null;
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
    await removeEntry(entry.id);
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
              onClick={() => setConfirmOpen(true)}
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
