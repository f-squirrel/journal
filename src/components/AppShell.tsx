import type { ReactNode } from "react";
import { AppBar, Box, Button, Stack, Toolbar, Typography } from "@mui/material";
import { useLocation, useNavigate } from "react-router-dom";
import { useAppState } from "../context/AppStateContext";

export default function AppShell({ children }: { children: ReactNode }) {
  const navigate = useNavigate();
  const location = useLocation();
  const { logout } = useAppState();

  const isEntries = location.pathname === "/entries";

  return (
    <Box sx={{ display: "flex", flexDirection: "column", minHeight: "100vh" }}>
      <AppBar
        position="static"
        color="inherit"
        elevation={0}
        sx={{ borderBottom: 1, borderColor: "divider" }}
      >
        <Toolbar sx={{ gap: 2 }}>
          <Typography variant="h6" sx={{ fontWeight: 700 }}>
            Journal
          </Typography>
          <Stack direction="row" spacing={1} sx={{ flex: 1 }}>
            <Button variant={isEntries ? "outlined" : "text"} onClick={() => navigate("/entries")}>
              Entries
            </Button>
            <Button variant="text" onClick={() => navigate("/entries/new/edit")}>
              New entry
            </Button>
          </Stack>
          <Button
            color="inherit"
            onClick={() => {
              logout();
              navigate("/login");
            }}
          >
            Log out
          </Button>
        </Toolbar>
      </AppBar>
      <Box component="main" sx={{ flex: 1, p: 3, overflowY: "auto" }}>
        {children}
      </Box>
    </Box>
  );
}
