import { useEffect, useRef, useState } from "react";
import {
  Box,
  Button,
  Collapse,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import { useAppState } from "../context/AppStateContext";
import { useSettings } from "../context/SettingsContext";
import { errorMessage } from "../utils/errors";
import { rem } from "./chrome";

/// The same rule as `DeviceSettings::validate` in Rust.
const APP_KEY = /^[A-Za-z0-9]{1,64}$/;

/// Dropbox's own settings: the app key. Shown in Settings › Sync and on the
/// login screen, where a device with no journal yet joins one; the key is a
/// device setting, so it works while the journal is locked.
export default function DropboxSettings() {
  const { settings, update } = useSettings();
  const { sync, refreshProviders } = useAppState();
  const saved = settings.sync_providers.dropbox.app_key ?? "";
  // Keeps its own text while focused or unsaved, so a settings reload can't
  // wipe a half-typed key.
  const [text, setText] = useState(saved);
  const [error, setError] = useState("");
  const [helpOpen, setHelpOpen] = useState(false);
  const editing = useRef(false);
  const connected = sync.state !== "off";

  useEffect(() => {
    if (!editing.current) setText(saved);
  }, [saved]);

  async function save() {
    editing.current = false;
    const key = text.trim();
    if (key === saved) {
      setText(key);
      setError("");
      return;
    }
    if (key !== "" && !APP_KEY.test(key)) {
      setError("Letters and digits only.");
      return;
    }
    setError("");
    try {
      await update({
        sync_providers: {
          ...settings.sync_providers,
          dropbox: { app_key: key === "" ? null : key },
        },
      });
      await refreshProviders();
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  return (
    <Stack spacing={1.5} data-testid="dropbox-settings">
      <Typography variant="subtitle2">Dropbox</Typography>
      <TextField
        label="App key"
        value={text}
        disabled={connected}
        error={error !== ""}
        helperText={
          error || (connected ? "Disconnect to change the app key." : undefined)
        }
        sx={{ maxWidth: rem(280) }}
        slotProps={{
          htmlInput: {
            "data-testid": "dropbox-app-key",
            maxLength: 64,
            autoCapitalize: "off",
            autoCorrect: "off",
            spellCheck: false,
          },
        }}
        onFocus={() => {
          editing.current = true;
        }}
        onChange={(event) => {
          editing.current = true;
          setText(event.target.value);
          setError("");
        }}
        onBlur={() => void save()}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void save();
          }
        }}
      />
      <Typography variant="caption" color="text.secondary">
        Use the same key on every device.
      </Typography>
      <Box data-testid="dropbox-key-help">
        <Button
          size="small"
          onClick={() => setHelpOpen((open) => !open)}
          aria-expanded={helpOpen}
          startIcon={helpOpen ? <ExpandLessIcon /> : <ExpandMoreIcon />}
        >
          How to get a key
        </Button>
        <Collapse in={helpOpen} unmountOnExit>
          <Typography
            variant="body2"
            color="text.secondary"
            component="div"
            sx={{ pl: 1 }}
          >
            <ol style={{ margin: 0, paddingLeft: rem(20) }}>
              <li>
                Register an app at <code>dropbox.com/developers</code> with
                &ldquo;App folder&rdquo; access.
              </li>
              <li>
                Add the redirect URI <code>http://127.0.0.1:53682/</code>.
              </li>
              <li>
                Tick the permissions <code>files.metadata.read</code>,{" "}
                <code>files.content.read</code>,{" "}
                <code>files.content.write</code> and{" "}
                <code>account_info.read</code>.
              </li>
              <li>Copy the app key here.</li>
            </ol>
          </Typography>
        </Collapse>
      </Box>
    </Stack>
  );
}
