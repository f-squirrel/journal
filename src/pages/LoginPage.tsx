import { useEffect, useState, type FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import {
  Alert,
  Avatar,
  Box,
  Button,
  CircularProgress,
  Divider,
  Paper,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import AutoStoriesIcon from "@mui/icons-material/AutoStories";
import { useAppState } from "../context/AppStateContext";
import { hasVault } from "../api/entries";

/// The shortest passphrase the create-vault form accepts, stated to the user
/// in the field's helper text (and asserted against that rendered text by
/// `e2e/create-vault.spec.ts`, so the number lives here only).
///
/// It guards the sealed key file, which since ADR 0007 holds the only copy of
/// the key every entry is encrypted to: a passphrase short enough to brute
/// force is the whole journal.
const MIN_PASSPHRASE_LENGTH = 12;

export default function LoginPage() {
  const { login, sync, syncLoaded, providers, connectSync } = useAppState();
  const navigate = useNavigate();
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  // null until `has_vault` answers, and for as long as it keeps rejecting:
  // "unknown", never "returning user".
  const [vaultExists, setVaultExists] = useState<boolean | null>(null);
  // Bumped by "Try again". Without a way to re-run the check, one rejection
  // leaves `vaultExists` null — and the form disabled — for the life of the
  // page, with a restart as the only way back into the app.
  const [checkAttempt, setCheckAttempt] = useState(0);
  const [checkFailed, setCheckFailed] = useState(false);
  // Joining a journal that already lives in the cloud (see `joinFrom`).
  const [joining, setJoining] = useState<string | null>(null);
  const [notice, setNotice] = useState("");

  useEffect(() => {
    let cancelled = false;
    hasVault()
      .then((exists) => {
        if (cancelled) return;
        setCheckFailed(false);
        setError("");
        setVaultExists(exists);
      })
      .catch((err) => {
        if (cancelled) return;
        console.error("Failed to check for a vault:", err);
        // Told, not just logged: a user who cannot be told whether a vault
        // exists is handed a form that stays disabled, and needs to know why.
        setCheckFailed(true);
        setError("Couldn't check for an existing journal.");
      });
    // Cancelled like `AppStateContext`'s loads: with a retry the effect can
    // re-run, and a slow first attempt must not land after a second one.
    return () => {
      cancelled = true;
    };
  }, [checkAttempt]);

  // What `handleSubmit` enforces: strict unless we know a vault exists, so a
  // `hasVault()` rejection fails safe — creating one is the irreversible
  // branch, and it must not be taken with the checks off.
  const isNewVault = vaultExists !== true;
  // What the form *shows*: neither branch until we actually know. Letting
  // `isNewVault` drive the rendering would flash the confirm field, the
  // warning and "Create vault" on every launch until `has_vault` answers,
  // which trains a returning user to ignore the one warning that matters.
  const pending = vaultExists === null;

  /// The second device's way in: connect the account, which runs a pass, and
  /// the journal's key arrives with it. Asking again then finds a vault, and
  /// the form turns into a sign-in. The cloud may also hold no journal yet, and
  /// then the form stays a create form, now with sync connected.
  async function joinFrom(provider: (typeof providers)[number]) {
    setError("");
    setNotice("");
    setJoining(provider.id);
    try {
      await connectSync(provider.id);
      const exists = await hasVault();
      setVaultExists(exists);
      if (!exists) {
        setNotice(
          `Connected to ${provider.name}, but it holds no journal yet. Create one here and it will be uploaded.`,
        );
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setJoining(null);
    }
  }

  // Offered only when it is known that there is no journal here and no account
  // connected: a returning user has nothing to join, and a connected one has
  // already joined.
  const canJoin =
    vaultExists === false &&
    syncLoaded &&
    sync.state === "off" &&
    providers.length > 0;

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();

    // Not redundant with the disabled button: `form.requestSubmit()` ignores
    // that attribute, as does any engine that treats a disabled default button
    // as absent and falls back to implicit submission. The invariant belongs
    // in the handler, not in a rendered attribute.
    if (pending) {
      return;
    }

    if (!password) {
      setError("Enter your passphrase to continue.");
      return;
    }

    // Creating a vault is the one irreversible step this form takes: a typo
    // becomes the real passphrase, and by the warning below there is nothing
    // to recover it with. Returning users need neither check — they already
    // have a passphrase, and its length is not theirs to change here. Only a
    // *known* existing vault skips them, so an unanswered check is strict.
    if (isNewVault) {
      if (password.length < MIN_PASSPHRASE_LENGTH) {
        setError(
          `Use a passphrase of at least ${MIN_PASSPHRASE_LENGTH} characters.`,
        );
        return;
      }
      if (password !== confirmation) {
        setError("The two passphrases don't match.");
        return;
      }
    }

    setError("");
    setSubmitting(true);
    try {
      await login(password);
      navigate("/entries");
    } catch (err) {
      // Shown verbatim: the backend's `#[error]` strings are the user-facing
      // copy, so there is no mapping table here to fall out of step with them.
      setError(String(err));
      // Asked again because the answer can change under the form: a journal
      // arriving from the cloud mid-create turns it into a sign-in, and a
      // form still asking for a new passphrase would send the next attempt
      // at a key sealed under another one. Best-effort — the error above is
      // what the user needs, and a rejection leaves the form as it was.
      hasVault()
        .then(setVaultExists)
        .catch(() => {});
    } finally {
      // In a `finally` because `login` now rejects: re-enabling the button
      // only on the way out of the `try` strands the user on this screen
      // after any failure.
      setSubmitting(false);
    }
  }

  return (
    <Box
      sx={{
        minHeight: "100vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        p: 4,
      }}
    >
      <Paper
        elevation={0}
        sx={{
          p: 4,
          width: "100%",
          maxWidth: 380,
          textAlign: "center",
          bgcolor: "tonal.surfaceContainerLow",
        }}
      >
        <Avatar
          sx={{
            bgcolor: "tonal.primaryContainer",
            color: "tonal.onPrimaryContainer",
            width: 56,
            height: 56,
            mx: "auto",
            mb: 2,
          }}
        >
          <AutoStoriesIcon />
        </Avatar>
        <Typography variant="h4" fontWeight={700} gutterBottom>
          Journal
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
          {pending
            ? "Checking your journal…"
            : isNewVault
              ? "Create a passphrase to secure your journal"
              : "Sign in to continue"}
        </Typography>
        <Box component="form" onSubmit={handleSubmit} noValidate>
          <Stack spacing={2} textAlign="left">
            <TextField
              label="Passphrase"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete={
                pending
                  ? "off"
                  : isNewVault
                    ? "new-password"
                    : "current-password"
              }
              helperText={
                pending || !isNewVault
                  ? undefined
                  : `Use at least ${MIN_PASSPHRASE_LENGTH} characters.`
              }
              fullWidth
            />
            {!pending && isNewVault && (
              <TextField
                label="Confirm passphrase"
                type="password"
                value={confirmation}
                onChange={(event) => setConfirmation(event.target.value)}
                autoComplete="new-password"
                fullWidth
              />
            )}
            {!pending && isNewVault && (
              <Alert severity="warning">
                There's no way to recover this passphrase or your entries if you
                forget it: it unlocks the key file your entries are encrypted
                to, and that key exists nowhere else. Keep it somewhere safe.
              </Alert>
            )}
            {notice && <Alert severity="info">{notice}</Alert>}
            {error && (
              <Alert
                severity="error"
                action={
                  checkFailed ? (
                    // Must not be a submit button: it sits above the submit one
                    // in the Stack, so it would become the form's default
                    // button. MUI's ButtonBase defaults to type="button".
                    <Button
                      color="inherit"
                      size="small"
                      onClick={() => {
                        setError("");
                        setCheckAttempt((n) => n + 1);
                      }}
                    >
                      Try again
                    </Button>
                  ) : undefined
                }
              >
                {error}
              </Alert>
            )}
            {/* Disabled while the answer is unknown, so the form cannot be
                submitted against an unknown vault state at all. The fields
                stay live: typing early is fine, submitting early is not. */}
            <Button
              type="submit"
              variant="contained"
              size="large"
              disableElevation
              fullWidth
              disabled={submitting || pending}
              startIcon={
                pending ? (
                  <CircularProgress size={16} color="inherit" />
                ) : undefined
              }
            >
              {pending ? "Checking…" : isNewVault ? "Create vault" : "Sign in"}
            </Button>
            {canJoin && (
              <>
                <Divider>or</Divider>
                <Typography variant="body2" color="text.secondary">
                  Already have a journal in the cloud? Connect the account it is
                  in to use it here.
                </Typography>
                {providers.map((provider) => (
                  <Button
                    key={provider.id}
                    variant="outlined"
                    fullWidth
                    onClick={() => joinFrom(provider)}
                    disabled={joining !== null || submitting}
                  >
                    {joining === provider.id
                      ? `Waiting for ${provider.name}…`
                      : `Connect ${provider.name}`}
                  </Button>
                ))}
              </>
            )}
          </Stack>
        </Box>
      </Paper>
    </Box>
  );
}
