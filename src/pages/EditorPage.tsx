import { useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { Box, Button, Paper, Stack, TextField, Typography } from "@mui/material";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import AppShell from "../components/AppShell";
import { useAppState } from "../context/AppStateContext";

export default function EditorPage() {
  const { id } = useParams<{ id: string }>();
  const isNew = id === "new";
  const { findEntry, upsertEntry } = useAppState();
  const navigate = useNavigate();
  const existing = useMemo(() => (isNew ? undefined : findEntry(id ?? "")), [isNew, id, findEntry]);

  const [title, setTitle] = useState(existing?.title ?? "");
  const [markdown, setMarkdown] = useState(existing?.markdown ?? "");

  if (!isNew && !existing) {
    return (
      <AppShell>
        <Typography color="text.secondary" textAlign="center" sx={{ mt: 6 }}>
          Entry not found.
        </Typography>
      </AppShell>
    );
  }

  function handleSave() {
    const entryId = existing?.id ?? String(Date.now());
    upsertEntry({
      id: entryId,
      title: title.trim() || "Untitled entry",
      updatedAt: new Date().toISOString().slice(0, 10),
      markdown,
    });
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
          <Button variant="contained" disableElevation onClick={handleSave}>
            Save
          </Button>
        </Stack>
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
