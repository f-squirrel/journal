import { navigate } from "../router";
import { renderShell } from "../layout";
import { findEntry, upsertEntry } from "../state";
import { renderMarkdown } from "../markdown";

export function renderEditor(container: HTMLElement, params: { id: string }): void {
  const isNew = params.id === "new";
  const existing = isNew ? undefined : findEntry(params.id);
  const content = renderShell(container, "editor");

  if (!isNew && !existing) {
    content.innerHTML = `<p class="empty-state">Entry not found.</p>`;
    return;
  }

  const wrapper = document.createElement("div");
  wrapper.className = "editor-screen";
  wrapper.innerHTML = `
    <div class="editor-toolbar">
      <input id="entry-title" class="title-input" placeholder="Entry title" value="${existing?.title ?? ""}" />
      <div class="editor-actions">
        <button data-action="cancel">Cancel</button>
        <button data-action="save" class="primary">Save</button>
      </div>
    </div>
    <div class="editor-split">
      <textarea id="entry-body" class="markdown-input" placeholder="Write in Markdown...">${existing?.markdown ?? ""}</textarea>
      <div id="entry-preview" class="markdown-live-preview"></div>
    </div>
  `;
  content.appendChild(wrapper);

  const titleInput = wrapper.querySelector<HTMLInputElement>("#entry-title")!;
  const bodyInput = wrapper.querySelector<HTMLTextAreaElement>("#entry-body")!;
  const livePreview = wrapper.querySelector<HTMLDivElement>("#entry-preview")!;

  const updatePreview = () => {
    livePreview.innerHTML = renderMarkdown(bodyInput.value);
  };
  updatePreview();
  bodyInput.addEventListener("input", updatePreview);

  wrapper.querySelector('[data-action="cancel"]')?.addEventListener("click", () => {
    navigate(existing ? `/entries/${existing.id}` : "/entries");
  });

  wrapper.querySelector('[data-action="save"]')?.addEventListener("click", () => {
    const id = existing?.id ?? String(Date.now());
    const title = titleInput.value.trim() || "Untitled entry";
    upsertEntry({
      id,
      title,
      updatedAt: new Date().toISOString().slice(0, 10),
      markdown: bodyInput.value,
    });
    navigate(`/entries/${id}`);
  });
}
