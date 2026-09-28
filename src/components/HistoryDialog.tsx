import { useEffect, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  List,
  ListItemButton,
  ListItemText,
  Stack,
  Typography,
} from "@mui/material";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  getHistoryEntry,
  listEntryHistory,
  restoreEntryVersion,
} from "../api/entries";
import { rem, TYPE } from "../components/chrome";
import type { EntryHeader, JournalEntry } from "../types";
import { formatEntryHeading, formatTime } from "../utils/date";
import { errorMessage } from "../utils/errors";
import { useSettings } from "../context/SettingsContext";

type Busy = "list" | "preview" | "restore" | null;

/// Lists an entry's past versions, lets the user preview one, and restore it.
///
/// Modeled on `ExportDialog`'s loading/error/dialog-open shape. Restoring
/// itself reaches the vault directly (`restoreEntryVersion`), not through
/// `useAppState`'s `upsertEntry` — a restore is a read-then-conditional-write
/// the backend does atomically with its own snapshot, not an ordinary save —
/// so the caller adopts the result into the entries list itself
/// (`onRestored`).
export default function HistoryDialog({
  open,
  onClose,
  entryId,
  entryTitle,
  expectedUpdatedAt,
  onRestored,
}: {
  open: boolean;
  onClose: () => void;
  entryId: string;
  /// For the confirm prompt on the selected row.
  entryTitle: string;
  /// The `updated_at` the pane last saw for the *live* entry, flushed just
  /// before this dialog opened. Passed through to `restoreEntryVersion` as
  /// its precondition.
  expectedUpdatedAt: string | null;
  onRestored: (entry: JournalEntry) => void;
}) {
  const { settings } = useSettings();
  const [versions, setVersions] = useState<EntryHeader[] | null>(null);
  const [selected, setSelected] = useState<EntryHeader | null>(null);
  const [preview, setPreview] = useState<JournalEntry | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) return;
    setBusy("list");
    setError("");
    setVersions(null);
    setSelected(null);
    setPreview(null);
    listEntryHistory(entryId)
      .then((headers) => setVersions(headers))
      .catch((err) => setError(errorMessage(err)))
      .finally(() => setBusy(null));
  }, [open, entryId]);

  function selectVersion(header: EntryHeader) {
    setSelected(header);
    setPreview(null);
    setBusy("preview");
    setError("");
    getHistoryEntry(entryId, header.updated_at)
      .then((entry) => setPreview(entry))
      .catch((err) => setError(errorMessage(err)))
      .finally(() => setBusy(null));
  }

  async function restore() {
    if (!selected) return;
    setBusy("restore");
    setError("");
    try {
      const restored = await restoreEntryVersion(
        entryId,
        selected.updated_at,
        expectedUpdatedAt,
      );
      onRestored(restored);
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  }

  return (
    <Dialog
      open={open}
      onClose={busy === null ? onClose : undefined}
      fullWidth
      maxWidth="sm"
      aria-labelledby="history-dialog-title"
    >
      <DialogTitle id="history-dialog-title">Version history</DialogTitle>
      <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {error && <Alert severity="error">{error}</Alert>}
        {versions !== null && versions.length === 0 ? (
          <Typography color="text.secondary" sx={{ py: 2 }}>
            No earlier versions of this entry yet.
          </Typography>
        ) : (
          <Stack
            direction={{ xs: "column", sm: "row" }}
            spacing={2}
            sx={{ minHeight: rem(240) }}
          >
            <List
              dense
              sx={{
                width: { xs: "100%", sm: rem(220) },
                flexShrink: 0,
                overflowY: "auto",
                maxHeight: rem(320),
              }}
            >
              {(versions ?? []).map((header) => (
                <ListItemButton
                  key={header.updated_at}
                  selected={selected?.updated_at === header.updated_at}
                  onClick={() => selectVersion(header)}
                >
                  <ListItemText
                    primary={header.title}
                    secondary={`${formatEntryHeading(header.updated_at)} · ${formatTime(header.updated_at, settings.time_format)}`}
                    slotProps={{
                      primary: { fontSize: TYPE.row, noWrap: true },
                      secondary: { fontSize: TYPE.meta },
                    }}
                  />
                </ListItemButton>
              ))}
            </List>
            <Stack sx={{ flex: 1, minWidth: 0 }} spacing={1}>
              <Box
                data-testid="history-preview"
                className="markdown-body"
                sx={{ overflowY: "auto", maxHeight: rem(320) }}
              >
                {preview ? (
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {preview.markdown}
                  </ReactMarkdown>
                ) : (
                  selected && (
                    <Typography color="text.secondary">Loading…</Typography>
                  )
                )}
              </Box>
              {preview && (
                <Typography variant="caption" color="text.secondary">
                  Restoring replaces {entryTitle.trim() || "this entry"}'s
                  current text — it will be kept as a version too.
                </Typography>
              )}
            </Stack>
          </Stack>
        )}
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={onClose} disabled={busy === "restore"}>
          Cancel
        </Button>
        {selected && (
          <Button
            variant="contained"
            disableElevation
            onClick={restore}
            disabled={busy !== null || !preview}
          >
            {busy === "restore" ? "Restoring…" : "Restore this version"}
          </Button>
        )}
      </DialogActions>
    </Dialog>
  );
}
