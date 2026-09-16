import { useEffect, useState, type FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import { Alert, Avatar, Box, Button, Paper, Stack, TextField, Typography } from "@mui/material";
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
  const { login } = useAppState();
  const navigate = useNavigate();
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  // null while unknown, which renders the "returning user" copy.
  const [vaultExists, setVaultExists] = useState<boolean | null>(null);

  useEffect(() => {
    hasVault()
      .then(setVaultExists)
      .catch((err) => console.error("Failed to check for a vault:", err));
  }, []);

  const isNewVault = vaultExists === false;

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();

    if (!password) {
      setError("Enter your passphrase to continue.");
      return;
    }

    // Creating a vault is the one irreversible step this form takes: a typo
    // becomes the real passphrase, and by the warning below there is nothing
    // to recover it with. Returning users need neither check — they already
    // have a passphrase, and its length is not theirs to change here.
    if (isNewVault) {
      if (password.length < MIN_PASSPHRASE_LENGTH) {
        setError(`Use a passphrase of at least ${MIN_PASSPHRASE_LENGTH} characters.`);
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
    } finally {
      // In a `finally` because `login` now rejects: re-enabling the button
      // only on the way out of the `try` strands the user on this screen
      // after any failure.
      setSubmitting(false);
    }
  }

  return (
    <Box sx={{ minHeight: "100vh", display: "flex", alignItems: "center", justifyContent: "center", p: 4 }}>
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
          {isNewVault ? "Create a passphrase to secure your journal" : "Sign in to continue"}
        </Typography>
        <Box component="form" onSubmit={handleSubmit} noValidate>
          <Stack spacing={2} textAlign="left">
            <TextField
              label="Passphrase"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete={isNewVault ? "new-password" : "current-password"}
              helperText={isNewVault ? `Use at least ${MIN_PASSPHRASE_LENGTH} characters.` : undefined}
              fullWidth
            />
            {isNewVault && (
              <TextField
                label="Confirm passphrase"
                type="password"
                value={confirmation}
                onChange={(event) => setConfirmation(event.target.value)}
                autoComplete="new-password"
                fullWidth
              />
            )}
            {isNewVault && (
              <Alert severity="warning">
                There's no way to recover this passphrase or your entries if you forget it:
                it unlocks the key file your entries are encrypted to, and that key exists
                nowhere else. Keep it somewhere safe.
              </Alert>
            )}
            {error && <Alert severity="error">{error}</Alert>}
            <Button type="submit" variant="contained" size="large" disableElevation fullWidth disabled={submitting}>
              {isNewVault ? "Create vault" : "Sign in"}
            </Button>
          </Stack>
        </Box>
      </Paper>
    </Box>
  );
}
