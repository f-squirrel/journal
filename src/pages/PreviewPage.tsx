import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { Box, Button, Paper, Stack, Typography } from "@mui/material";
import ArrowBackIcon from "@mui/icons-material/ArrowBack";
import EditIcon from "@mui/icons-material/Edit";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import AppShell from "../components/AppShell";
import { useAppState } from "../context/AppStateContext";
import type { JournalEntry } from "../types";
import { formatUpdatedAt } from "../utils/date";

export default function PreviewPage() {
  const { id } = useParams<{ id: string }>();
  const { loadEntry } = useAppState();
  const navigate = useNavigate();
  const [entry, setEntry] = useState<JournalEntry | undefined>(undefined);
  const [loading, setLoading] = useState(true);

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

  return (
    <AppShell>
      <Box sx={{ maxWidth: 720, mx: "auto" }}>
        <Stack direction="row" justifyContent="space-between" sx={{ mb: 3 }}>
          <Button startIcon={<ArrowBackIcon />} onClick={() => navigate("/entries")}>
            Back
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
        <Typography variant="h4" fontWeight={700}>
          {entry.title}
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
          Updated {formatUpdatedAt(entry.updatedAt)}
        </Typography>
        <Paper
          elevation={0}
          sx={{ p: { xs: 2.5, sm: 4 }, bgcolor: "tonal.surfaceContainerLow" }}
          className="markdown-body"
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{entry.markdown}</ReactMarkdown>
        </Paper>
      </Box>
    </AppShell>
  );
}
