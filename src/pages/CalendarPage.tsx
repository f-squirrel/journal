import { createContext, useContext, useMemo, useState } from "react";
import {
  Box,
  Button,
  IconButton,
  List,
  ListSubheader,
  Stack,
  Typography,
  useMediaQuery,
  useTheme,
} from "@mui/material";
import ArrowBackIcon from "@mui/icons-material/ArrowBack";
import { DateCalendar } from "@mui/x-date-pickers/DateCalendar";
import {
  PickersDay,
  type PickersDayProps,
} from "@mui/x-date-pickers/PickersDay";
import { useNavigate } from "react-router-dom";
import { HEADER_HEIGHT } from "../components/chrome";
import EntryRow from "../components/EntryRow";
import { useAppState } from "../context/AppStateContext";
import { useNow } from "../hooks/useNow";
import type { EntryHeader } from "../types";
import {
  dayKeyOf,
  entriesByDay,
  formatEntryHeading,
  formatYearsAgo,
  onThisDay,
} from "../utils/date";

/// The days that have entries, for `MarkedDay` to look itself up in. It reaches
/// the grid through context because the slot is a component type, not an
/// element: passing the set as a prop would mean defining the slot inside the
/// page, and a new component identity on every render remounts every day.
const MarksContext = createContext<Map<string, EntryHeader[]>>(new Map());

/// A day in the grid: MUI's own day, with a dot under it when the day has
/// entries. The dot is decorative — the count is in the panel below — so it is
/// hidden from the accessibility tree and the button keeps `PickersDay`'s
/// name, keyboard handling and ARIA.
///
/// Days from the neighbouring months are not marked: they are not the month on
/// screen, and the month they belong to marks them when it is.
function MarkedDay(props: PickersDayProps) {
  const marks = useContext(MarksContext);
  const marked = !props.outsideCurrentMonth && marks.has(dayKeyOf(props.day));

  return (
    <PickersDay {...props}>
      {props.day.toLocaleDateString(undefined, { day: "numeric" })}
      {marked && (
        <Box
          component="span"
          aria-hidden
          data-testid="day-dot"
          sx={{
            position: "absolute",
            bottom: 3,
            left: "50%",
            width: 4,
            height: 4,
            ml: "-2px",
            borderRadius: "50%",
            // On the selected day the button itself is primary, so the dot
            // takes the colour drawn on top of it.
            bgcolor: props.selected ? "primary.contrastText" : "primary.main",
          }}
        />
      )}
    </PickersDay>
  );
}

/// A heading for a run of rows, styled like the list column's month headers.
function GroupHeading({ children }: { children: string }) {
  return (
    <ListSubheader
      disableSticky
      sx={{
        bgcolor: "transparent",
        color: "text.secondary",
        fontSize: 11,
        fontWeight: 700,
        letterSpacing: 0.8,
        textTransform: "uppercase",
        lineHeight: 2.6,
      }}
    >
      {children}
    </ListSubheader>
  );
}

/// The calendar: a month grid marking every day with entries, the entries of
/// the day picked in it, and what the journal held on today's date in earlier
/// years.
///
/// Everything here is derived from the headers `AppStateContext` already
/// holds, so nothing is decrypted for it and nothing needs clearing when the
/// journal locks — `lockSession` empties `entries`, and this page goes with it.
/// Tapping a day only selects it; opening an entry is the row's own click.
export default function CalendarPage() {
  const { entries } = useAppState();
  const navigate = useNavigate();
  const theme = useTheme();
  const compact = useMediaQuery(theme.breakpoints.down("md"));

  const [selected, setSelected] = useState(() => new Date());
  const byDay = useMemo(() => entriesByDay(entries), [entries]);
  // Re-read each minute so a window left open past midnight moves on to the
  // new day's memories.
  const now = useNow(true, 60_000);
  const memories = useMemo(
    () => onThisDay(entries, new Date(now)),
    [entries, now],
  );

  const dayEntries = byDay.get(dayKeyOf(selected)) ?? [];

  return (
    <Stack data-testid="calendar-page" sx={{ height: "100%", minHeight: 0 }}>
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
        <Typography variant="h6" sx={{ fontSize: 17 }}>
          Calendar
        </Typography>
        <Box sx={{ flexGrow: 1 }} />
        {/* Always live, never disabled on today: it puts the grid back on this
            month as well as selecting the day, and after browsing to 2019 that
            is the whole point — a selection that is already today says nothing
            about which month is on screen. A fresh `Date` each time is what
            makes that work: the grid follows `value` by identity, so reusing
            the selected day's object would select today and leave the grid
            where it was. */}
        <Button size="small" onClick={() => setSelected(new Date())}>
          Today
        </Button>
      </Stack>

      <Box sx={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
        <Stack sx={{ maxWidth: 720, mx: "auto", py: 2, gap: 3 }}>
          <Stack direction="row" sx={{ flexWrap: "wrap", gap: 2 }}>
            <MarksContext.Provider value={byDay}>
              {/* No slide transition between months: the outgoing month stays
                  mounted through it and shadows the one on screen (ADR 0013). */}
              <DateCalendar
                value={selected}
                onChange={(day) => day && setSelected(day)}
                reduceAnimations
                slots={{ day: MarkedDay }}
              />
            </MarksContext.Provider>

            <Box sx={{ flex: "1 1 280px", minWidth: 0 }}>
              <Typography variant="subtitle1" component="h2" sx={{ px: 2 }}>
                {formatEntryHeading(selected.toISOString())}
              </Typography>
              {dayEntries.length === 0 ? (
                <Typography
                  variant="body2"
                  color="text.secondary"
                  sx={{ px: 2, pt: 1 }}
                >
                  No entries on this day.
                </Typography>
              ) : (
                <List disablePadding sx={{ pt: 1 }}>
                  {dayEntries.map((entry) => (
                    <EntryRow key={entry.id} entry={entry} />
                  ))}
                </List>
              )}
            </Box>
          </Stack>

          <Box>
            <Typography
              variant="overline"
              component="h2"
              color="text.secondary"
              sx={{ px: 2 }}
            >
              On this day
            </Typography>
            {memories.length === 0 ? (
              <Typography variant="body2" color="text.secondary" sx={{ px: 2 }}>
                Nothing from earlier years on this day.
              </Typography>
            ) : (
              <List disablePadding>
                {memories.map((group) => [
                  <GroupHeading key={`h${group.year}`}>
                    {`${formatYearsAgo(group.yearsAgo)} · ${group.year}`}
                  </GroupHeading>,
                  ...group.entries.map((entry) => (
                    <EntryRow key={entry.id} entry={entry} />
                  )),
                ])}
              </List>
            )}
          </Box>
        </Stack>
      </Box>
    </Stack>
  );
}
