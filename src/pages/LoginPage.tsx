import { useEffect, useState, type FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import { Alert, Avatar, Box, Button, Paper, Stack, TextField, Typography } from "@mui/material";
import AutoStoriesIcon from "@mui/icons-material/AutoStories";
import { useAppState } from "../context/AppStateContext";
import { hasVault } from "../api/entries";

export default function LoginPage() {
  const { login } = useAppState();
  const navigate = useNavigate();
  const [password, setPassword] = useState("");
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
              fullWidth
            />
            {isNewVault && (
              <Alert severity="warning">
                There's no way to recover this passphrase or your entries if you forget it.
                Make sure to keep it somewhere safe.
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
