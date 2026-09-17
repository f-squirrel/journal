import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { Alert, Box, Button, Paper, Stack, TextField, Typography } from "@mui/material";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import AppShell from "../components/AppShell";
import { useAppState } from "../context/AppStateContext";
import type { JournalEntry } from "../types";
import { errorMessage } from "../utils/errors";
import { newEntryId } from "../utils/id";

export default function EditorPage() {
  const { id } = useParams<{ id: string }>();
  const isNew = id === "new";
  const { loadEntry, upsertEntry } = useAppState();
  const navigate = useNavigate();

  const [existing, setExisting] = useState<JournalEntry | undefined>(undefined);
  const [loading, setLoading] = useState(!isNew);
  const [loadError, setLoadError] = useState("");
  const [saveError, setSaveError] = useState("");
  const [saving, setSaving] = useState(false);
  const [title, setTitle] = useState("");
  const [markdown, setMarkdown] = useState("");

  useEffect(() => {
    if (isNew) {
      setExisting(undefined);
      setTitle("");
      setMarkdown("");
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setLoadError("");
    loadEntry(id ?? "")
      .then((loaded) => {
        if (cancelled) return;
        setExisting(loaded);
        setTitle(loaded?.title ?? "");
        setMarkdown(loaded?.markdown ?? "");
        setLoading(false);
      })
      .catch((err) => {
        if (cancelled) return;
        setLoadError(errorMessage(err));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [id, isNew, loadEntry]);

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

  if (!isNew && !existing) {
    return (
      <AppShell>
        <Typography color="text.secondary" textAlign="center" sx={{ mt: 6 }}>
          Entry not found.
        </Typography>
      </AppShell>
    );
  }

  async function handleSave() {
    setSaving(true);
    setSaveError("");
    // Inside the `try`: generating the id can throw, and a throw out here
    // would be an unhandled rejection with a Save button that does nothing.
    let entryId: string;
    try {
      entryId = existing?.id ?? newEntryId();
      await upsertEntry({
        id: entryId,
        title: title.trim() || "Untitled entry",
        updated_at: new Date().toISOString(),
        markdown,
      });
    } catch (err) {
      // Nothing reached disk, so stay put with the draft intact.
      setSaveError(errorMessage(err));
      return;
    } finally {
      setSaving(false);
    }
    navigate(`/entries/${entryId}`);
  }

  return (
    <AppShell>
      <Stack sx={{ height: "100%" }} spacing={2}>
        <Stack direction="row" spacing={2} alignItems="center">
          <TextField
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            placeholder="Entry title"
            variant="outlined"
            sx={{ flex: 1, "& input": { fontSize: "1.2rem", fontWeight: 600 } }}
          />
          <Button onClick={() => navigate(existing ? `/entries/${existing.id}` : "/entries")}>Cancel</Button>
          <Button variant="contained" disableElevation onClick={handleSave} disabled={saving}>
            Save
          </Button>
        </Stack>
        {saveError && <Alert severity="error">{saveError}</Alert>}
        <Box
          sx={{
            flex: 1,
            display: "grid",
            gridTemplateColumns: { xs: "1fr", md: "1fr 1fr" },
            gap: 2,
            minHeight: "60vh",
          }}
        >
          <TextField
            value={markdown}
            onChange={(event) => setMarkdown(event.target.value)}
            placeholder="Write in Markdown..."
            multiline
            fullWidth
            minRows={18}
            sx={{
              height: "100%",
              "& .MuiInputBase-root": {
                height: "100%",
                alignItems: "flex-start",
                fontFamily: "monospace",
                bgcolor: "tonal.surfaceContainerLow",
              },
              "& textarea": { height: "100% !important" },
              "& fieldset": { border: "none" },
            }}
          />
          <Paper
            elevation={0}
            sx={{ p: 2.5, overflowY: "auto", bgcolor: "tonal.surfaceContainerLow" }}
            className="markdown-body"
          >
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{markdown}</ReactMarkdown>
          </Paper>
        </Box>
      </Stack>
    </AppShell>
  );
}
