import { useState } from "react";
import { Alert, AlertTitle, Button, Stack } from "@mui/material";
import { useAppState } from "../context/AppStateContext";
import { errorMessage } from "../utils/errors";
import ExportDialog from "./ExportDialog";

/// What Settings › Sync says when the cloud holds a different journal than this
/// device: the two cannot be merged, so the user keeps this one without sync or
/// switches to the cloud's.
///
/// `pending` is the connect case: the login was granted but not saved, so
/// "Cancel" is offered. The halted case (`key_mismatch`) has the login saved
/// already, and Disconnect stays where Settings has it.
///
/// Switching locks the journal and ends on the login screen, so a successful
/// switch has nothing more to do here.
export default function DifferentJournalPanel({
  provider,
  pending,
  onDone,
}: {
  provider: string;
  pending: boolean;
  onDone: () => void;
}) {
  const { switchToCloudJournal, discardPendingConnection } = useAppState();
  const [exportOpen, setExportOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function switchJournal() {
    setSwitching(true);
    setError("");
    try {
      await switchToCloudJournal();
    } catch (err) {
      // The backend guarantees nothing changed: the journal is still open.
      setError(errorMessage(err));
      setSwitching(false);
    }
  }

  async function cancel() {
    setBusy(true);
    setError("");
    try {
      await discardPendingConnection();
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <Stack spacing={1.5} data-testid="different-journal">
      <Alert severity="warning">
        <AlertTitle>This cloud holds a different journal</AlertTitle>
        This device and {provider} each hold their own journal, with its own
        passphrase. They can't be merged. You can keep using this journal
        without sync, or switch this device to the one in {provider}.
      </Alert>
      {confirming && (
        <Alert severity="warning">
          This device's journal is moved to a backup folder in the app's data
          directory. Nothing is deleted, and the backup stays locked with this
          journal's passphrase. Next you sign in with the passphrase of the
          journal in {provider}.
        </Alert>
      )}
      {error && <Alert severity="error">{error}</Alert>}
      <Stack direction="row" spacing={1} useFlexGap flexWrap="wrap">
        {confirming ? (
          <>
            <Button
              color="error"
              variant="contained"
              disableElevation
              data-testid="use-cloud-journal"
              onClick={() => void switchJournal()}
              disabled={switching}
            >
              {switching
                ? `Switching to the journal in ${provider}…`
                : "Move this journal aside and switch"}
            </Button>
            <Button onClick={() => setConfirming(false)} disabled={switching}>
              Back
            </Button>
          </>
        ) : (
          <>
            <Button variant="outlined" onClick={() => setExportOpen(true)}>
              Export this journal first
            </Button>
            <Button
              variant="outlined"
              color="error"
              onClick={() => setConfirming(true)}
              disabled={busy}
            >
              Use the cloud's journal…
            </Button>
            {pending && (
              <Button onClick={() => void cancel()} disabled={busy}>
                Cancel
              </Button>
            )}
          </>
        )}
      </Stack>
      <ExportDialog open={exportOpen} onClose={() => setExportOpen(false)} />
    </Stack>
  );
}
