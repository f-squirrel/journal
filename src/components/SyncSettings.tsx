import { useState } from "react";
import {
  Alert,
  Box,
  Button,
  MenuItem,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { useAppState } from "../context/AppStateContext";
import { useSettings } from "../context/SettingsContext";
import type { ProviderInfo } from "../types";
import { formatAgo } from "../utils/date";
import { errorMessage } from "../utils/errors";
import { pendingCount, syncSummary } from "../utils/sync";
import { rem } from "./chrome";
import DropboxSettings from "./DropboxSettings";

type Busy = "connect" | "sync" | "disconnect" | null;

/// Minutes offered for the periodic sync check. `null` is "only when I sync".
const SYNC_INTERVAL_OPTIONS: readonly (number | null)[] = [1, 5, 15, 60, null];
/// `null` travels as this string between the select and the setting.
const NEVER = "never";

function minutesLabel(minutes: number): string {
  return minutes === 1 ? "1 minute" : `${minutes} minutes`;
}

/// `options` plus `current`, if it isn't one of them, sorted back into place
/// with `null` last: the backend accepts any value in its range, so one set
/// some other way still shows as selected.
function withCurrent(
  options: readonly (number | null)[],
  current: number | null,
): (number | null)[] {
  if (current === null || options.includes(current)) return [...options];
  const numbers = options.filter((o): o is number => o !== null);
  const sorted: (number | null)[] = [...numbers, current].sort((a, b) => a - b);
  return [...sorted, null];
}

/// Settings › Sync: the connection and its status, when to check, the
/// providers, and each provider's own settings.
///
/// The one place sync asks anything of the user, so it also carries the
/// reasons: the failure message the backend gave, and what a disconnect leaves
/// behind.
export default function SyncSettings() {
  const { sync, providers, connectSync, disconnectSync, syncNow } =
    useAppState();
  const { settings, update } = useSettings();
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
  const now = Date.now();

  async function run(kind: Exclude<Busy, null>, work: () => Promise<void>) {
    setBusy(kind);
    setError("");
    try {
      await work();
    } catch (err) {
      setError(errorMessage(err));
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

  function providerStatus(p: ProviderInfo) {
    if (!p.available)
      return <Typography variant="body2">Coming soon</Typography>;
    const isConnected = connected && sync.provider_id === p.id;
    if (isConnected && !needsLogin) {
      return <Typography variant="body2">Connected</Typography>;
    }
    if (!p.configured) {
      return <Typography variant="body2">Needs an app key</Typography>;
    }
    const label =
      connecting === p.id
        ? `Waiting for ${p.name}…`
        : isConnected
          ? "Sign in"
          : "Connect";
    return (
      <Button
        size="small"
        variant="outlined"
        onClick={() => void connect(p.id)}
        disabled={busy !== null}
      >
        {label}
      </Button>
    );
  }

  return (
    <Stack
      component="section"
      id="sync"
      data-testid="sync-settings"
      spacing={3}
    >
      <Typography variant="overline" component="h2" color="text.secondary">
        Sync
      </Typography>

      <Stack spacing={1}>
        <Typography variant="body2" data-testid="sync-status">
          {connected
            ? `Connected to ${provider}. ${syncSummary(sync, now)}`
            : "Sync is off. Keep a copy of your journal in the cloud and read it on your other devices; entries are encrypted on this device before they are uploaded."}
        </Typography>
        {sync.last_synced_at && sync.state !== "synced" && (
          <Typography variant="caption" color="text.secondary">
            Last synced {formatAgo(sync.last_synced_at, now)}.
          </Typography>
        )}
        {sync.message && (
          <Alert severity={sync.state === "offline" ? "warning" : "error"}>
            {sync.message}
          </Alert>
        )}
        {connecting && (
          <Alert severity="info">
            Finish signing in to{" "}
            {providers.find((p) => p.id === connecting)?.name} in your browser,
            then return here.
          </Alert>
        )}
        {error && <Alert severity="error">{error}</Alert>}
      </Stack>

      {connected &&
        (confirmingDisconnect ? (
          <Stack spacing={1.5}>
            <Alert severity={unsent > 0 ? "warning" : "info"}>
              {unsent > 0
                ? `${unsent === 1 ? "1 change has" : `${unsent} changes have`} not been uploaded. They stay on this device but will not reach ${provider}.`
                : `This stops syncing with ${provider}. Your journal on this device is not touched, and neither is the copy in ${provider}.`}
            </Alert>
            <Stack direction="row" spacing={1}>
              <Button
                onClick={() => setConfirmingDisconnect(false)}
                disabled={busy !== null}
              >
                Keep syncing
              </Button>
              <Button
                color="error"
                onClick={() => void disconnect()}
                disabled={busy !== null}
              >
                Disconnect
              </Button>
            </Stack>
          </Stack>
        ) : (
          <Stack direction="row" spacing={1}>
            {!needsLogin && (
              <Button
                variant="contained"
                disableElevation
                onClick={() => void run("sync", syncNow)}
                disabled={busy !== null}
              >
                {busy === "sync" ? "Syncing…" : "Sync now"}
              </Button>
            )}
            <Button
              color="error"
              onClick={() => setConfirmingDisconnect(true)}
              disabled={busy !== null}
            >
              Disconnect…
            </Button>
          </Stack>
        ))}

      <TextField
        select
        label="Check for sync every"
        disabled={sync.state === "off"}
        helperText={
          sync.state === "off"
            ? "Connect a cloud account to change this."
            : undefined
        }
        sx={{ maxWidth: rem(280) }}
        value={String(settings.sync_interval_minutes ?? NEVER)}
        onChange={(event) => {
          const raw = event.target.value;
          update({
            sync_interval_minutes: raw === NEVER ? null : Number(raw),
          }).catch((err) => setError(errorMessage(err)));
        }}
      >
        {withCurrent(SYNC_INTERVAL_OPTIONS, settings.sync_interval_minutes).map(
          (minutes) => (
            <MenuItem key={minutes ?? NEVER} value={String(minutes ?? NEVER)}>
              {minutes === null ? "Only when I sync" : minutesLabel(minutes)}
            </MenuItem>
          ),
        )}
      </TextField>

      <Stack spacing={1}>
        <Typography variant="subtitle2">Providers</Typography>
        {providers.map((p) => (
          <Stack
            key={p.id}
            direction="row"
            alignItems="center"
            justifyContent="space-between"
            spacing={2}
            data-testid={`provider-${p.id}`}
            sx={{ opacity: p.available ? 1 : 0.6, minHeight: rem(36) }}
          >
            <Typography variant="body2">{p.name}</Typography>
            <Box sx={{ flexShrink: 0 }}>{providerStatus(p)}</Box>
          </Stack>
        ))}
      </Stack>

      <DropboxSettings />
    </Stack>
  );
}
