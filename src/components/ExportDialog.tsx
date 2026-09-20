import { useState } from "react";
import {
  Alert,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Stack,
  Typography,
} from "@mui/material";
import { chooseExportDirectory, exportEntries } from "../api/entries";
import { useAppState } from "../context/AppStateContext";

type Busy = "choose" | "export" | null;

const entriesOf = (n: number) => `${n} ${n === 1 ? "entry" : "entries"}`;

/// Writes the journal out as plain markdown files: the backup, and the way out
/// of the app.
///
/// The one place the app puts the journal on disk unencrypted, so nothing is
/// written until the person has picked a folder *and* been shown its name next
/// to a plain statement of what that means. It stays mounted while closed, like
/// `SyncDialog`, and forgets its state only once the close has finished
/// animating, so the last thing on screen is not rewritten as it fades.
export default function ExportDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const { entries } = useAppState();
  const [directory, setDirectory] = useState<string | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState("");
  const [exported, setExported] = useState<number | null>(null);

  async function run(kind: Exclude<Busy, null>, work: () => Promise<void>) {
    setBusy(kind);
    setError("");
    try {
      await work();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  }

  const choose = () =>
    run("choose", async () => {
      // `null` is a cancelled picker: keep whatever was chosen before.
      const chosen = await chooseExportDirectory();
      if (chosen) setDirectory(chosen);
    });

  const exportNow = () =>
    run("export", async () => {
      if (directory) setExported(await exportEntries(directory));
    });

  function reset() {
    setDirectory(null);
    setExported(null);
    setError("");
  }

  // A write in flight is not something to walk away from without seeing how it
  // ended.
  const close = () => {
    if (busy !== "export") onClose();
  };

  return (
    <Dialog
      open={open}
      onClose={close}
      fullWidth
      maxWidth="xs"
      aria-labelledby="export-dialog-title"
      slotProps={{ transition: { onExited: reset } }}
    >
      <DialogTitle id="export-dialog-title">Export journal</DialogTitle>
      <DialogContent>
        <Stack spacing={2}>
          {exported !== null ? (
            <Alert severity="success">
              Exported {entriesOf(exported)} to {directory}.
            </Alert>
          ) : directory === null ? (
            <Typography variant="body2" color="text.secondary">
              Save every entry as a markdown file, named for its date and title,
              in a folder you choose. Use it as a backup, or to take your
              journal to another app.
            </Typography>
          ) : (
            <>
              <Typography variant="body2" color="text.secondary">
                Export {entriesOf(entries.length)} to
              </Typography>
              <Typography
                data-testid="export-directory"
                sx={{ fontFamily: "monospace", wordBreak: "break-all" }}
              >
                {directory}
              </Typography>
              <Alert severity="warning">
                The exported files are not encrypted. Anyone, and any program,
                that can read this folder can read your whole journal. Keep it
                somewhere you trust, and delete it when you are done.
              </Alert>
            </>
          )}
          {error && <Alert severity="error">{error}</Alert>}
        </Stack>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        {exported !== null ? (
          <Button onClick={close}>Close</Button>
        ) : (
          <>
            <Button onClick={close} disabled={busy === "export"}>
              Cancel
            </Button>
            <Button onClick={choose} disabled={busy !== null}>
              {directory === null ? "Choose folder…" : "Change folder"}
            </Button>
            {directory !== null && (
              <Button
                variant="contained"
                disableElevation
                onClick={exportNow}
                disabled={busy !== null}
              >
                {busy === "export" ? "Exporting…" : "Export"}
              </Button>
            )}
          </>
        )}
      </DialogActions>
    </Dialog>
  );
}
