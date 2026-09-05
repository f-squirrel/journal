function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function inline(text: string): string {
  return escapeHtml(text)
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/\*(.+?)\*/g, "<em>$1</em>")
    .replace(/`(.+?)`/g, "<code>$1</code>")
    .replace(/\[(.+?)\]\((.+?)\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>');
}

/**
 * Dependency-free markdown-to-HTML renderer, good enough to scaffold the
 * preview screens. Swap for a proper markdown library before shipping.
 */
export function renderMarkdown(markdown: string): string {
  const lines = markdown.split("\n");
  const html: string[] = [];
  let listType: "ul" | "ol" | null = null;
  let inQuote = false;

  const closeList = () => {
    if (listType) {
      html.push(listType === "ul" ? "</ul>" : "</ol>");
      listType = null;
    }
  };
  const closeQuote = () => {
    if (inQuote) {
      html.push("</blockquote>");
      inQuote = false;
    }
  };

  for (const rawLine of lines) {
    const line = rawLine.trimEnd();
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    const ordered = /^\d+\.\s+(.*)$/.exec(line);
    const unordered = /^[-*]\s+(.*)$/.exec(line);
    const quote = /^>\s?(.*)$/.exec(line);

    if (heading) {
      closeList();
      closeQuote();
      const level = heading[1].length;
      html.push(`<h${level}>${inline(heading[2])}</h${level}>`);
    } else if (unordered) {
      closeQuote();
      if (listType !== "ul") {
        closeList();
        html.push("<ul>");
        listType = "ul";
      }
      html.push(`<li>${inline(unordered[1])}</li>`);
    } else if (ordered) {
      closeQuote();
      if (listType !== "ol") {
        closeList();
        html.push("<ol>");
        listType = "ol";
      }
      html.push(`<li>${inline(ordered[1])}</li>`);
    } else if (quote) {
      closeList();
      if (!inQuote) {
        html.push("<blockquote>");
        inQuote = true;
      }
      html.push(`<p>${inline(quote[1])}</p>`);
    } else if (line === "") {
      closeList();
      closeQuote();
    } else {
      closeList();
      closeQuote();
      html.push(`<p>${inline(line)}</p>`);
    }
  }
  closeList();
  closeQuote();
  return html.join("\n");
}
