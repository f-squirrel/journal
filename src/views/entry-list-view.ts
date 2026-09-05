import { navigate } from "../router";
import { renderShell } from "../layout";
import { state } from "../state";

export function renderEntryList(container: HTMLElement): void {
  const content = renderShell(container, "entries");

  const list = document.createElement("div");
  list.className = "entry-list";

  if (state.entries.length === 0) {
    list.innerHTML = `<p class="empty-state">No entries yet. Create your first one.</p>`;
  } else {
    for (const entry of state.entries) {
      const item = document.createElement("button");
      item.className = "entry-card";
      item.innerHTML = `
        <span class="entry-card-title">${entry.title}</span>
        <span class="entry-card-date">${entry.updatedAt}</span>
      `;
      item.addEventListener("click", () => navigate(`/entries/${entry.id}`));
      list.appendChild(item);
    }
  }

  content.appendChild(list);
}
