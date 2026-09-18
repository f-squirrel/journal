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
import { useAppState } from "../context/AppStateContext";
import { formatAgo } from "../utils/date";
import { pendingCount, syncSummary } from "../utils/sync";

type Busy = "connect" | "sync" | "disconnect" | null;

/// Where the cloud account is connected, checked, retried and disconnected.
///
/// The one place sync asks anything of the user, so it also carries the
/// reasons: the failure message the backend gave, and what a disconnect leaves
/// behind. It stays mounted while closed, so a connection still waiting on the
/// browser is still shown as waiting when the dialog is opened again.
export default function SyncDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const { sync, providers, connectSync, disconnectSync, syncNow } =
    useAppState();
  const [busy, setBusy] = useState<Busy>(null);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [confirmingDisconnect, setConfirmingDisconnect] = useState(false);

  const connected = sync.state !== "off";
  // A login the provider refuses is connected in the backend's eyes but needs
  // the browser again; the same button that connects also reconnects.
  const needsLogin = sync.state === "off" || sync.state === "reauth";
  const unsent = pendingCount(sync);
  const provider = sync.provider ?? "the cloud";

  async function run(kind: Exclude<Busy, null>, work: () => Promise<void>) {
    setBusy(kind);
    setError("");
    try {
      await work();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
      setConnecting(null);
    }
  }

  const connect = (id: string) =>
    run("connect", () => {
      setConnecting(id);
      return connectSync(id);
    });

  const disconnect = () =>
    run("disconnect", async () => {
      await disconnectSync();
      setConfirmingDisconnect(false);
    });

  function close() {
    setConfirmingDisconnect(false);
    onClose();
  }

  return (
    <Dialog
      open={open}
      onClose={close}
      fullWidth
      maxWidth="xs"
      aria-labelledby="sync-dialog-title"
    >
      <DialogTitle id="sync-dialog-title">Cloud sync</DialogTitle>
      <DialogContent>
        <Stack spacing={2}>
          {confirmingDisconnect ? (
            <Alert severity={unsent > 0 ? "warning" : "info"}>
              {unsent > 0
                ? `${unsent === 1 ? "1 change has" : `${unsent} changes have`} not been uploaded. They stay on this device but will not reach ${provider}.`
                : `This stops syncing with ${provider}. Your journal on this device is not touched, and neither is the copy in ${provider}.`}
            </Alert>
          ) : (
            <>
              <Typography variant="body2" color="text.secondary">
                {connected
                  ? `Connected to ${provider}. `
                  : "Keep a copy of your journal in the cloud and read it on your other devices. Entries are encrypted on this device before they are uploaded."}
                {connected && syncSummary(sync, Date.now())}
              </Typography>
              {sync.last_synced_at && sync.state !== "synced" && (
                <Typography variant="caption" color="text.secondary">
                  Last synced {formatAgo(sync.last_synced_at, Date.now())}.
                </Typography>
              )}
              {sync.message && (
                <Alert
                  severity={sync.state === "offline" ? "warning" : "error"}
                >
                  {sync.message}
                </Alert>
              )}
              {connecting && (
                <Alert severity="info">
                  Finish signing in to{" "}
                  {providers.find((p) => p.id === connecting)?.name} in your
                  browser, then return here.
                </Alert>
              )}
            </>
          )}
          {error && <Alert severity="error">{error}</Alert>}
        </Stack>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2, flexWrap: "wrap", gap: 1 }}>
        {confirmingDisconnect ? (
          <>
            <Button
              onClick={() => setConfirmingDisconnect(false)}
              disabled={busy !== null}
            >
              Keep syncing
            </Button>
            <Button color="error" onClick={disconnect} disabled={busy !== null}>
              Disconnect
            </Button>
          </>
        ) : (
          <>
            {connected && (
              <Button
                color="error"
                onClick={() => setConfirmingDisconnect(true)}
                disabled={busy !== null}
                sx={{ mr: "auto" }}
              >
                Disconnect
              </Button>
            )}
            <Button onClick={close}>Close</Button>
            {needsLogin ? (
              providers.map((p) => (
                <Button
                  key={p.id}
                  variant="contained"
                  disableElevation
                  onClick={() => connect(p.id)}
                  disabled={busy !== null}
                >
                  {connecting === p.id
                    ? `Waiting for ${p.name}…`
                    : `${connected ? "Sign in to" : "Connect"} ${p.name}`}
                </Button>
              ))
            ) : (
              <Button
                variant="contained"
                disableElevation
                onClick={() => run("sync", syncNow)}
                disabled={busy !== null}
              >
                {busy === "sync" ? "Syncing…" : "Sync now"}
              </Button>
            )}
          </>
        )}
      </DialogActions>
    </Dialog>
  );
}
