import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Box,
  Button,
  IconButton,
  MenuItem,
  Stack,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
  useMediaQuery,
  useTheme,
} from "@mui/material";
import ArrowBackIcon from "@mui/icons-material/ArrowBack";
import BrightnessAutoIcon from "@mui/icons-material/BrightnessAuto";
import DarkModeIcon from "@mui/icons-material/DarkMode";
import KeyboardIcon from "@mui/icons-material/Keyboard";
import LightModeIcon from "@mui/icons-material/LightMode";
import { useLocation, useNavigate } from "react-router-dom";
import ShortcutsDialog from "../components/ShortcutsDialog";
import SyncSettings from "../components/SyncSettings";
import { HEADER_HEIGHT, READING_WIDTH, TYPE, rem } from "../components/chrome";
import { useAppState } from "../context/AppStateContext";
import { useSettings } from "../context/SettingsContext";
import type { ColorMode, SettingsPatch, TimeFormat, WeekStart } from "../types";
import { errorMessage } from "../utils/errors";

const ENTRY_TEMPLATE_LIMIT = 10_000;

/// Minutes offered for the idle lock. `null` is "Never".
const IDLE_LOCK_OPTIONS: readonly (number | null)[] = [1, 5, 15, 30, 60, null];

/// A `Select`-flavoured `TextField` whose value is `number | null`: MUI's
/// `Select` wants a primitive it can compare by `===`, and `null` fails that
/// silently (the menu just shows nothing selected), so `null` travels as the
/// sentinel string `"never"` between the field and the caller.
const NEVER = "never";

function minutesLabel(minutes: number): string {
  return minutes === 1 ? "1 minute" : `${minutes} minutes`;
}

/// `options` plus `current`, if it isn't already one of them — sorted back
/// into place, `null` (always "Never") last. The backend
/// accepts any value in its range (1..=240), not just the handful
/// this page offers by default, so a value set some other way (an older
/// build, a future one, directly through `update_settings`) still shows as a
/// selected option instead of leaving the field blank.
function withCurrent(
  options: readonly (number | null)[],
  current: number | null,
): (number | null)[] {
  if (current === null || options.includes(current)) return [...options];
  const numbers = options.filter((o): o is number => o !== null);
  const sorted: (number | null)[] = [...numbers, current].sort((a, b) => a - b);
  return [...sorted, null];
}

export default function SettingsPage() {
  const theme = useTheme();
  const compact = useMediaQuery(theme.breakpoints.down("md"));
  const navigate = useNavigate();
  const location = useLocation();
  const { registerFlush } = useAppState();
  const { settings, update } = useSettings();
  const [error, setError] = useState("");
  const [shortcutsOpen, setShortcutsOpen] = useState(false);

  // The sidebar's sync button lands here with `scrollTo` in the navigation
  // state; `location.key` changes on every navigation, so it also works when
  // this page is already open.
  useEffect(() => {
    const target = (location.state as { scrollTo?: string } | null)?.scrollTo;
    if (target) document.getElementById(target)?.scrollIntoView();
  }, [location.key, location.state]);

  const change = useCallback(
    async (patch: SettingsPatch) => {
      setError("");
      try {
        await update(patch);
      } catch (err) {
        // The context has already reverted the optimistic value by the time
        // this rejection reaches here (`SettingsContext`'s `reload()`); this
        // alert is the one thing it doesn't do on its own.
        setError(errorMessage(err));
      }
    },
    [update],
  );

  // --- the entry template -------------------------------------------------
  //
  // Keeps its own text while focused, and only takes the context's value
  // (an update from another device, or this one's own save round-tripping)
  // while it isn't — so a `sync://changed` reload mid-edit can't wipe a
  // half-typed template. Saves on blur, and also whenever the field goes away
  // with unsaved text: React does not fire `blur` for a focused element that
  // is removed, so an idle lock or a navigation would otherwise lose the
  // edit. See the plan, WP5, step 1.
  //
  // A save is decided by `dirtyRef`, not by comparing the field's text to
  // `settings.entry_template`: the field can be focused (so it hasn't taken a
  // newer value the context just received) without the user having typed
  // anything, and diffing against the stale display text would then write
  // that stale text back over the newer one — on this device, and, once the
  // journal half's per-field stamping picks it up, on every other. Only an
  // actual edit (`onChange`) sets `dirtyRef`; a successful save clears it.
  const [entryTemplate, setEntryTemplateState] = useState(
    settings.entry_template,
  );
  const entryTemplateRef = useRef(entryTemplate);
  const setEntryTemplate = useCallback((next: string) => {
    entryTemplateRef.current = next;
    setEntryTemplateState(next);
  }, []);
  const focused = useRef(false);
  const dirtyRef = useRef(false);
  const updateRef = useRef(update);
  updateRef.current = update;

  useEffect(() => {
    if (!focused.current && !dirtyRef.current) {
      setEntryTemplate(settings.entry_template);
    }
  }, [settings.entry_template, setEntryTemplate]);

  const flushTemplate = useCallback(async (): Promise<boolean> => {
    if (!dirtyRef.current) return true;
    try {
      await updateRef.current({ entry_template: entryTemplateRef.current });
      dirtyRef.current = false;
      return true;
    } catch (err) {
      // Keeps the user's typed text (and stays dirty) rather than reverting
      // it to the old value: the other controls revert on a failed change
      // because there is nothing to lose in re-picking a select option, but
      // overwriting up to 10,000 typed characters is not a fair trade for
      // matching that behaviour.
      setError(errorMessage(err));
      return false;
    }
  }, []);

  // `lockWhenIdle` and log out both save unsaved drafts through here before
  // the vault locks (`AppStateContext.flushAll`), the same registry the entry
  // pane's autosave uses.
  useEffect(() => registerFlush(flushTemplate), [registerFlush, flushTemplate]);

  // Saves on unmount — navigating away from this page — if the field is
  // still dirty. Fire-and-forget: `update` queues the write in
  // `SettingsContext`, so it still reaches the backend once this page is
  // gone; the rejection is swallowed rather than surfaced, since there is no
  // longer a page to show it on. Runs once, on unmount only, reading through
  // refs so it always sees the latest text and the latest `update`.
  useEffect(() => {
    return () => {
      if (dirtyRef.current) {
        updateRef
          .current({ entry_template: entryTemplateRef.current })
          .catch(() => {
            // Nothing to show it to; see above.
          });
      }
    };
  }, []);

  return (
    <Stack data-testid="settings-page" sx={{ height: "100%", minHeight: 0 }}>
      <Stack
        direction="row"
        alignItems="center"
        spacing={1}
        sx={{
          px: 1.5,
          height: HEADER_HEIGHT,
          borderBottom: 1,
          borderColor: "divider",
          flexShrink: 0,
        }}
      >
        {compact && (
          <IconButton aria-label="Back" onClick={() => navigate("/entries")}>
            <ArrowBackIcon />
          </IconButton>
        )}
        <Typography variant="h6" sx={{ fontSize: TYPE.header }}>
          Settings
        </Typography>
      </Stack>

      <Box sx={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
        <Stack
          sx={{
            maxWidth: READING_WIDTH,
            mx: "auto",
            px: { xs: 2.5, sm: 5 },
            py: { xs: 3, sm: 5 },
            gap: 5,
          }}
        >
          {error && <Alert severity="error">{error}</Alert>}

          <Stack component="section" spacing={3}>
            <Typography
              variant="overline"
              component="h2"
              color="text.secondary"
            >
              This device
            </Typography>

            <Stack spacing={1}>
              <Typography variant="body2">Colour mode</Typography>
              <ToggleButtonGroup
                aria-label="Colour mode"
                exclusive
                value={settings.color_mode}
                onChange={(_event, next: ColorMode | null) => {
                  if (next) void change({ color_mode: next });
                }}
              >
                <ToggleButton value="system">
                  <BrightnessAutoIcon fontSize="small" sx={{ mr: 1 }} />
                  System
                </ToggleButton>
                <ToggleButton value="light">
                  <LightModeIcon fontSize="small" sx={{ mr: 1 }} />
                  Light
                </ToggleButton>
                <ToggleButton value="dark">
                  <DarkModeIcon fontSize="small" sx={{ mr: 1 }} />
                  Dark
                </ToggleButton>
              </ToggleButtonGroup>
            </Stack>

            <TextField
              select
              label="Lock when idle"
              sx={{ maxWidth: rem(280) }}
              value={String(settings.idle_lock_minutes ?? NEVER)}
              onChange={(event) => {
                const raw = event.target.value;
                void change({
                  idle_lock_minutes: raw === NEVER ? null : Number(raw),
                });
              }}
            >
              {withCurrent(IDLE_LOCK_OPTIONS, settings.idle_lock_minutes).map(
                (minutes) => (
                  <MenuItem
                    key={minutes ?? NEVER}
                    value={String(minutes ?? NEVER)}
                  >
                    {minutes === null ? "Never" : minutesLabel(minutes)}
                  </MenuItem>
                ),
              )}
            </TextField>

            <Box>
              <Button
                startIcon={<KeyboardIcon />}
                onClick={() => setShortcutsOpen(true)}
              >
                Keyboard shortcuts…
              </Button>
            </Box>
          </Stack>

          <SyncSettings />

          <Stack component="section" spacing={3}>
            <Typography
              variant="overline"
              component="h2"
              color="text.secondary"
            >
              This journal
            </Typography>
            <Typography variant="body2" color="text.secondary">
              These settings apply on every device.
            </Typography>

            <TextField
              select
              label="Week starts on"
              sx={{ maxWidth: rem(280) }}
              value={settings.week_starts_on}
              onChange={(event) => {
                void change({
                  week_starts_on: event.target.value as WeekStart,
                });
              }}
            >
              <MenuItem value="sunday">Sunday</MenuItem>
              <MenuItem value="monday">Monday</MenuItem>
              <MenuItem value="saturday">Saturday</MenuItem>
            </TextField>

            <TextField
              select
              label="Time format"
              sx={{ maxWidth: rem(280) }}
              value={settings.time_format}
              onChange={(event) => {
                void change({ time_format: event.target.value as TimeFormat });
              }}
            >
              <MenuItem value="locale">Automatic</MenuItem>
              <MenuItem value="h12">12-hour</MenuItem>
              <MenuItem value="h24">24-hour</MenuItem>
            </TextField>

            <TextField
              label="Entry template"
              multiline
              minRows={4}
              maxRows={12}
              // Keeps the label floated from mount: WebKitGTK doesn't
              // reliably re-open the outline's notch when the label
              // animates from placeholder to shrunk, leaving it struck
              // through by the border once this field (the only one here
              // that starts empty) gets a value.
              slotProps={{ inputLabel: { shrink: true } }}
              inputProps={{
                "data-testid": "entry-template",
                maxLength: ENTRY_TEMPLATE_LIMIT,
              }}
              helperText={`${entryTemplate.length} / ${ENTRY_TEMPLATE_LIMIT}`}
              value={entryTemplate}
              onFocus={() => {
                focused.current = true;
              }}
              onChange={(event) => {
                dirtyRef.current = true;
                setEntryTemplate(event.target.value);
              }}
              onBlur={() => {
                focused.current = false;
                if (dirtyRef.current) {
                  void flushTemplate();
                } else {
                  // Nothing was typed while focused, but the context's value
                  // may have moved on since (a reload this field ignored
                  // while it had focus) — catch up now that it's safe to.
                  setEntryTemplate(settings.entry_template);
                }
              }}
            />
          </Stack>
        </Stack>
      </Box>

      <ShortcutsDialog
        open={shortcutsOpen}
        onClose={() => setShortcutsOpen(false)}
      />
    </Stack>
  );
}
