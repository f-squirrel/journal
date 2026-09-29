import { useState, type KeyboardEvent } from "react";
import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  InputBase,
  Stack,
  Typography,
} from "@mui/material";
import { ACTIONS, type ActionId } from "../shortcuts/actions";
import { bindingFromEvent, formatBinding } from "../shortcuts/binding";
import { useShortcuts } from "../shortcuts/ShortcutsContext";
import { rem } from "./chrome";

/// Lists every action in `ACTIONS`, in registry order, with its current
/// chord, and lets each be rebound in place.
///
/// Opened by the `help.shortcuts` shortcut (registered in `JournalLayout`,
/// which owns `open`/`onClose` so the dialog survives the sidebar being
/// unmounted on phones) and by the "Keyboard shortcuts" rail button. The
/// **Close** button matches `ExportDialog`; without it this
/// dialog would have no focusable content at all.
///
/// **Change** turns a row's chord cell into a focused recorder ("Press
/// keys…"). The global shortcuts listener (`ShortcutsContext`) already skips
/// every keydown while any `[role="dialog"]` is open, so the recorder never
/// races the action it's capturing a new chord for. A bare `Escape` cancels
/// recording without also closing the dialog — `stopPropagation` beats MUI's
/// `Modal` `Escape`-closes-the-dialog handler, which is itself a bubble-phase
/// `onKeyDown` on the modal root (`useModal.js`), an ancestor of the recorder
/// in the React tree. Losing focus (a click elsewhere, including one of the
/// dialog's own buttons) cancels the same way.
export default function ShortcutsDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const { bindingFor, labelFor, setBinding, resetBinding, resetAll } =
    useShortcuts();
  const ids = Object.keys(ACTIONS) as ActionId[];

  // The one row currently capturing a chord, if any, and the refusal reason
  // for its last attempt. Only one row records at a time: starting to record
  // another, or any other interaction with the dialog, cancels this one via
  // the recorder's own `onBlur`.
  const [recordingId, setRecordingId] = useState<ActionId | null>(null);
  const [error, setError] = useState<string | null>(null);

  function cancelRecording() {
    setRecordingId(null);
    setError(null);
  }

  function handleRecorderKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    // Every key this field sees is being captured as a chord, not typed —
    // and `stopPropagation` is what keeps a bare `Escape` from also reaching
    // the dialog's own close-on-Escape handler.
    event.preventDefault();
    event.stopPropagation();
    const id = recordingId;
    if (id === null) return;

    if (
      event.key === "Escape" &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey &&
      !event.shiftKey
    ) {
      cancelRecording();
      return;
    }

    const binding = bindingFromEvent(event.nativeEvent);
    // A bare modifier press: keep waiting for the rest of the chord.
    if (binding === null) return;

    const refusal = setBinding(id, binding);
    if (refusal) {
      setError(refusal);
      return;
    }
    cancelRecording();
  }

  function handleClose() {
    cancelRecording();
    onClose();
  }

  return (
    <Dialog
      open={open}
      onClose={handleClose}
      fullWidth
      maxWidth="xs"
      aria-labelledby="shortcuts-dialog-title"
    >
      <DialogTitle id="shortcuts-dialog-title">Keyboard shortcuts</DialogTitle>
      <DialogContent>
        <Stack spacing={1.5} sx={{ py: 0.5 }}>
          {ids.map((id) => {
            const binding = bindingFor(id);
            const overridden = binding !== ACTIONS[id].default;
            const recording = recordingId === id;
            return (
              <Stack
                key={id}
                direction="row"
                flexWrap="wrap"
                alignItems="center"
                justifyContent="space-between"
                sx={{ rowGap: 0.5, columnGap: 2 }}
                data-testid={`shortcut-row-${id}`}
              >
                <Typography variant="body2">{labelFor(id)}</Typography>
                <Stack
                  direction="row"
                  flexWrap="wrap"
                  alignItems="center"
                  sx={{ rowGap: 0.5, columnGap: 1 }}
                >
                  {recording ? (
                    <InputBase
                      autoFocus
                      placeholder="Press keys…"
                      value=""
                      // Every keystroke is intercepted in `onKeyDown` and
                      // prevented, so nothing ever reaches the input's own
                      // value — this only silences React's controlled-input
                      // warning.
                      onChange={() => {}}
                      onKeyDown={handleRecorderKeyDown}
                      onBlur={cancelRecording}
                      inputProps={{
                        "aria-label": `New shortcut for ${labelFor(id)}`,
                        "data-testid": "shortcut-keys",
                      }}
                      sx={{
                        fontFamily: "monospace",
                        fontSize: rem(12),
                        width: rem(96),
                        color: "text.secondary",
                        bgcolor: "action.selected",
                        border: 1,
                        borderColor: "primary.main",
                        borderRadius: rem(4),
                        padding: `${rem(2)} ${rem(6)}`,
                        "& input": { textAlign: "center", padding: 0 },
                      }}
                    />
                  ) : (
                    <>
                      <Box
                        component="kbd"
                        data-testid="shortcut-keys"
                        sx={{
                          fontFamily: "monospace",
                          fontSize: rem(12),
                          color:
                            binding === null
                              ? "text.disabled"
                              : "text.secondary",
                          bgcolor: "action.hover",
                          border: 1,
                          borderColor: "divider",
                          borderRadius: rem(4),
                          padding: `${rem(2)} ${rem(6)}`,
                          whiteSpace: "nowrap",
                        }}
                      >
                        {binding === null ? "—" : formatBinding(binding)}
                      </Box>
                      <Button
                        size="small"
                        onClick={() => {
                          setError(null);
                          setRecordingId(id);
                        }}
                      >
                        Change
                      </Button>
                      <Button
                        size="small"
                        onClick={() => {
                          cancelRecording();
                          setBinding(id, null);
                        }}
                      >
                        Clear
                      </Button>
                      {overridden && (
                        <Button
                          size="small"
                          onClick={() => {
                            cancelRecording();
                            setError(resetBinding(id));
                          }}
                        >
                          Reset
                        </Button>
                      )}
                    </>
                  )}
                </Stack>
              </Stack>
            );
          })}
        </Stack>
        {error && (
          <Alert severity="error" sx={{ mt: 1.5 }}>
            {error}
          </Alert>
        )}
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2, justifyContent: "space-between" }}>
        <Button
          size="small"
          onClick={() => {
            cancelRecording();
            resetAll();
          }}
        >
          Reset all
        </Button>
        <Button onClick={handleClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}
