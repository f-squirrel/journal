import { useNavigate } from "react-router-dom";
import { Box, List, ListItemButton, ListItemText, Typography } from "@mui/material";
import AppShell from "../components/AppShell";
import { useAppState } from "../context/AppStateContext";

export default function EntryListPage() {
  const { entries } = useAppState();
  const navigate = useNavigate();

  return (
    <AppShell>
      <Box sx={{ maxWidth: 560, mx: "auto" }}>
        {entries.length === 0 ? (
          <Typography color="text.secondary" textAlign="center" sx={{ mt: 6 }}>
            No entries yet. Create your first one.
          </Typography>
        ) : (
          <List sx={{ display: "flex", flexDirection: "column", gap: 1.5 }}>
            {entries.map((entry) => (
              <ListItemButton
                key={entry.id}
                onClick={() => navigate(`/entries/${entry.id}`)}
                sx={{ bgcolor: "background.paper", borderRadius: 2, boxShadow: 1, py: 1.5, px: 2.5 }}
              >
                <ListItemText primary={entry.title} slotProps={{ primary: { fontWeight: 600 } }} />
                <Typography variant="caption" color="text.secondary">
                  {entry.updatedAt}
                </Typography>
              </ListItemButton>
            ))}
          </List>
        )}
      </Box>
    </AppShell>
  );
}
