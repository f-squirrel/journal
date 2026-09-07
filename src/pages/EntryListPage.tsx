import { useNavigate } from "react-router-dom";
import {
  Box,
  Button,
  Card,
  CardActionArea,
  CardContent,
  Fab,
  Stack,
  Typography,
} from "@mui/material";
import AddIcon from "@mui/icons-material/Add";
import NotesIcon from "@mui/icons-material/Notes";
import AppShell from "../components/AppShell";
import { useAppState } from "../context/AppStateContext";

export default function EntryListPage() {
  const { entries } = useAppState();
  const navigate = useNavigate();

  return (
    <AppShell>
      <Box sx={{ maxWidth: 720, mx: "auto" }}>
        <Stack direction="row" alignItems="center" justifyContent="space-between" sx={{ mb: 3 }}>
          <Typography variant="h5" sx={{ fontWeight: 700 }}>
            Entries
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {entries.length} {entries.length === 1 ? "entry" : "entries"}
          </Typography>
        </Stack>

        {entries.length === 0 ? (
          <Stack
            alignItems="center"
            spacing={2}
            sx={{
              mt: 8,
              py: 8,
              px: 4,
              borderRadius: 4,
              border: "1px dashed",
              borderColor: "divider",
              textAlign: "center",
            }}
          >
            <NotesIcon sx={{ fontSize: 40, color: "text.disabled" }} />
            <Typography color="text.secondary">No entries yet. Start your journal.</Typography>
            <Button
              variant="contained"
              disableElevation
              startIcon={<AddIcon />}
              onClick={() => navigate("/entries/new/edit")}
            >
              Write your first entry
            </Button>
          </Stack>
        ) : (
          <Stack spacing={1.5}>
            {entries.map((entry) => (
              <Card
                key={entry.id}
                elevation={0}
                sx={{ transition: "background-color 0.15s ease", "&:hover": { bgcolor: "tonal.surfaceContainer" } }}
              >
                <CardActionArea onClick={() => navigate(`/entries/${entry.id}`)}>
                  <CardContent sx={{ py: 2, px: 2.5 }}>
                    <Stack direction="row" justifyContent="space-between" alignItems="flex-start" spacing={2}>
                      <Box sx={{ minWidth: 0 }}>
                        <Typography sx={{ fontWeight: 600 }} noWrap>
                          {entry.title}
                        </Typography>
                      </Box>
                      <Typography variant="caption" color="text.secondary" sx={{ whiteSpace: "nowrap", pt: 0.5 }}>
                        {entry.updatedAt}
                      </Typography>
                    </Stack>
                  </CardContent>
                </CardActionArea>
              </Card>
            ))}
          </Stack>
        )}
      </Box>

      <Fab
        onClick={() => navigate("/entries/new/edit")}
        sx={{ position: "fixed", right: { xs: 20, sm: 32 }, bottom: { xs: 20, sm: 32 } }}
        aria-label="New entry"
      >
        <AddIcon />
      </Fab>
    </AppShell>
  );
}
