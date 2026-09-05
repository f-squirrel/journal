import { navigate } from "./router";
import { state } from "./state";

export type ActiveRoute = "entries" | "editor" | "preview";

export function renderShell(container: HTMLElement, activeRoute: ActiveRoute): HTMLElement {
  container.innerHTML = "";

  const shell = document.createElement("div");
  shell.className = "app-shell";

  shell.innerHTML = `
    <header class="app-header">
      <span class="app-title">Journal</span>
      <nav class="app-nav">
        <button data-nav="entries" class="nav-link${activeRoute === "entries" ? " active" : ""}">Entries</button>
        <button data-nav="new" class="nav-link${activeRoute === "editor" ? " active" : ""}">New entry</button>
      </nav>
      <button data-nav="logout" class="link-button">Log out</button>
    </header>
    <main class="app-content"></main>
  `;

  container.appendChild(shell);

  shell.querySelector('[data-nav="entries"]')?.addEventListener("click", () => navigate("/entries"));
  shell.querySelector('[data-nav="new"]')?.addEventListener("click", () => navigate("/entries/new/edit"));
  shell.querySelector('[data-nav="logout"]')?.addEventListener("click", () => {
    state.authenticated = false;
    navigate("/login");
  });

  return shell.querySelector(".app-content") as HTMLElement;
}
