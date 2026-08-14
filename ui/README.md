# ui

React + TypeScript + Vite frontend for the [journal](../README.md) desktop
app, built and served as the Tauri webview. See the root
[`README.md`](../README.md) for the project overview and
[`CLAUDE.md`](../CLAUDE.md) for build/dev commands.

## Commands

```bash
npm ci             # install dependencies
npm run dev        # Vite dev server (http://localhost:1420)
npm run build      # tsc -b && vite build
npm run lint       # oxlint
npm run preview    # preview a production build
```

Normally this is driven through `cargo tauri dev` / `cargo tauri build` from
the repo root, which run these scripts automatically.
