import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router-dom";
import { Alert, Avatar, Box, Button, Paper, Stack, TextField, Typography } from "@mui/material";
import AutoStoriesIcon from "@mui/icons-material/AutoStories";
import { useAppState } from "../context/AppStateContext";

export default function LoginPage() {
  const { login } = useAppState();
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState(false);

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!email.trim() || !password) {
      setError(true);
      return;
    }

    // TODO: replace with real authentication against the vault backend.
    login();
    navigate("/entries");
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
          Sign in to continue
        </Typography>
        <Box component="form" onSubmit={handleSubmit}>
          <Stack spacing={2} textAlign="left">
            <TextField
              label="Email"
              type="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              placeholder="you@example.com"
              autoComplete="email"
              fullWidth
              required
            />
            <TextField
              label="Passphrase"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete="current-password"
              fullWidth
              required
            />
            {error && <Alert severity="error">Enter an email and passphrase to continue.</Alert>}
            <Button type="submit" variant="contained" size="large" disableElevation fullWidth>
              Sign in
            </Button>
          </Stack>
        </Box>
      </Paper>
    </Box>
  );
}
