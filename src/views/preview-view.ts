import { navigate } from "../router";
import { renderShell } from "../layout";
import { findEntry } from "../state";
import { renderMarkdown } from "../markdown";

export function renderPreview(container: HTMLElement, params: { id: string }): void {
  const entry = findEntry(params.id);
  const content = renderShell(container, "preview");

  if (!entry) {
    content.innerHTML = `<p class="empty-state">Entry not found.</p>`;
    return;
  }

  const wrapper = document.createElement("article");
  wrapper.className = "preview-screen";
  wrapper.innerHTML = `
    <div class="preview-toolbar">
      <button data-action="back">&larr; Back</button>
      <button data-action="edit" class="primary">Edit</button>
    </div>
    <div class="preview-meta">
      <h1>${entry.title}</h1>
      <span class="preview-date">Updated ${entry.updatedAt}</span>
    </div>
    <div class="preview-body">${renderMarkdown(entry.markdown)}</div>
  `;
  content.appendChild(wrapper);

  wrapper.querySelector('[data-action="back"]')?.addEventListener("click", () => navigate("/entries"));
  wrapper
    .querySelector('[data-action="edit"]')
    ?.addEventListener("click", () => navigate(`/entries/${entry.id}/edit`));
}
