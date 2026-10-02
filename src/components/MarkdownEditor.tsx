import { useEffect, useImperativeHandle, useRef, type Ref } from "react";
import { Box, useTheme, type Theme } from "@mui/material";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  keymap,
  placeholder as cmPlaceholder,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { BODY_MIN_HEIGHT } from "./chrome";

/// Styles the markdown *as it is written*: a heading is large while its `#`
/// is still there to edit, a list marker is coloured, a quote is italic.
///
/// The syntax marks (`#`, `-`, `>`, `**`) are their own tokens — `@lezer/markdown`
/// tags them `processingInstruction` — so they can recede without being hidden.
/// Hiding them is what makes a WYSIWYG editor: then the document on screen and
/// the markdown on disk stop matching, and something has to serialize one back
/// into the other on every save.
function markdownHighlight(theme: Theme): HighlightStyle {
  const muted = theme.palette.text.secondary;
  const text = theme.palette.text.primary;
  const mono = '"Roboto Mono", ui-monospace, SFMono-Regular, Menlo, monospace';

  // Headings state their colour rather than inheriting it: `@lezer/markdown`
  // tags a heading's text with `heading` as well as `heading1`, and whichever
  // rule matches first wins outright — so a muted `heading` rule would take
  // the line's colour with it.
  return HighlightStyle.define([
    {
      tag: tags.heading1,
      fontSize: "1.6rem",
      fontWeight: "700",
      lineHeight: "1.35",
      color: text,
    },
    {
      tag: tags.heading2,
      fontSize: "1.3rem",
      fontWeight: "700",
      lineHeight: "1.4",
      color: text,
    },
    { tag: tags.heading3, fontSize: "1.12rem", fontWeight: "700", color: text },
    {
      tag: [tags.heading4, tags.heading5, tags.heading6],
      fontWeight: "700",
      color: text,
    },
    { tag: tags.strong, fontWeight: "700" },
    { tag: tags.emphasis, fontStyle: "italic" },
    { tag: tags.strikethrough, textDecoration: "line-through" },
    { tag: tags.quote, fontStyle: "italic", color: muted },
    { tag: tags.monospace, fontFamily: mono, fontSize: "0.92em" },
    { tag: tags.link, color: theme.palette.primary.main },
    { tag: tags.url, color: muted, fontSize: "0.92em" },
    // `tags.list` is the item's *text*, not its bullet — colouring it tints
    // the whole list. The bullet is a `processingInstruction`, below.
    {
      tag: tags.processingInstruction,
      color: muted,
      fontWeight: "400",
      fontSize: "1rem",
    },
  ]);
}

/// Everything that follows the app's light/dark mode, kept in one compartment
/// so a theme switch reconfigures the running editor instead of rebuilding it
/// and losing the selection and undo history.
function themeExtensions(theme: Theme): Extension {
  return [
    EditorView.theme(
      {
        "&": {
          color: theme.palette.text.primary,
          backgroundColor: "transparent",
          fontSize: "1rem",
        },
        "&.cm-focused": { outline: "none" },
        ".cm-scroller": { fontFamily: "inherit", lineHeight: "1.75" },
        ".cm-content": {
          fontFamily: theme.typography.entryFontFamily,
          padding: 0,
          caretColor: theme.palette.text.primary,
        },
        ".cm-line": { padding: 0 },
        ".cm-placeholder": { color: theme.palette.text.disabled },
      },
      { dark: theme.palette.mode === "dark" },
    ),
    syntaxHighlighting(markdownHighlight(theme)),
  ];
}

export interface MarkdownEditorHandle {
  /// Inserts `text` on its own line at the cursor as it is now.
  insertAtCursor(text: string): void;
}

/// Inserts `text` at `pos` on a line of its own: a newline before it unless
/// `pos` starts a line, and one after it unless `pos` ends one.
function insertOwnLine(view: EditorView, pos: number, text: string) {
  const at = Math.min(pos, view.state.doc.length);
  const line = view.state.doc.lineAt(at);
  const insert =
    (at === line.from ? "" : "\n") + text + (at === line.to ? "" : "\n");
  view.dispatch({
    changes: { from: at, insert },
    selection: { anchor: at + insert.length },
    scrollIntoView: true,
  });
}

const imageFiles = (files: FileList | null | undefined): File[] =>
  [...(files ?? [])].filter((file) => file.type.startsWith("image/"));

/// The entry body: one surface that is always editable and always styled, so
/// nothing reflows when focus arrives or leaves. What is on screen is the
/// markdown that gets encrypted — there is no second, rendered representation.
export default function MarkdownEditor({
  value,
  onChange,
  placeholder,
  autoFocus = false,
  onFiles,
  ref,
}: {
  value: string;
  onChange: (markdown: string) => void;
  placeholder: string;
  /// Set when the editor is opened by a deliberate switch into edit mode, so
  /// the caret is already where writing continues.
  autoFocus?: boolean;
  /// Called with the image files pasted or dropped on the editor. Returns the
  /// markdown to insert where they landed, or `null` to insert nothing (it
  /// has already said why). Text paste and text drags are left alone.
  onFiles?: (files: File[]) => Promise<string | null>;
  ref?: Ref<MarkdownEditorHandle>;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  const themes = useRef(new Compartment());
  const theme = useTheme();
  // Read by the editor's own callbacks, which outlive the render that made
  // them: `onChange` is rebuilt on every keystroke.
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const themeRef = useRef(theme);
  themeRef.current = theme;
  const onFilesRef = useRef(onFiles);
  onFilesRef.current = onFiles;
  // Where each in-flight import will insert, kept in step with every edit
  // made while it runs: importing takes a second or more and the user keeps
  // typing.
  const landing = useRef(new Set<{ pos: number }>());

  useImperativeHandle(ref, () => ({
    insertAtCursor(text) {
      const editor = view.current;
      if (editor) insertOwnLine(editor, editor.state.selection.main.head, text);
    },
  }));

  async function insertFiles(editor: EditorView, files: File[], pos: number) {
    const place = { pos };
    landing.current.add(place);
    let text: string | null = null;
    try {
      text = (await onFilesRef.current?.(files)) ?? null;
    } finally {
      landing.current.delete(place);
    }
    // Dropped if the editor went away meanwhile (another entry, read mode):
    // the saved image is then an orphan, which is accepted.
    if (text && view.current === editor) insertOwnLine(editor, place.pos, text);
  }

  useEffect(() => {
    const editor = new EditorView({
      state: EditorState.create({
        // `EntryPane` renders nothing until the entry has loaded, so this is
        // the real document rather than an empty one to be patched later.
        doc: value,
        extensions: [
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          markdown({ base: markdownLanguage }),
          EditorView.lineWrapping,
          cmPlaceholder(placeholder),
          themes.current.of(themeExtensions(themeRef.current)),
          EditorView.updateListener.of((update) => {
            if (!update.docChanged) return;
            for (const place of landing.current)
              place.pos = update.changes.mapPos(place.pos, 1);
            onChangeRef.current(update.state.doc.toString());
          }),
          EditorView.domEventHandlers({
            paste(event, editorView) {
              const files = imageFiles(event.clipboardData?.files);
              if (!files.length || !onFilesRef.current) return false;
              event.preventDefault();
              void insertFiles(
                editorView,
                files,
                editorView.state.selection.main.head,
              );
              return true;
            },
            drop(event, editorView) {
              const files = imageFiles(event.dataTransfer?.files);
              if (!files.length || !onFilesRef.current) return false;
              event.preventDefault();
              const pos =
                editorView.posAtCoords({
                  x: event.clientX,
                  y: event.clientY,
                }) ?? editorView.state.selection.main.head;
              void insertFiles(editorView, files, pos);
              return true;
            },
          }),
        ],
      }),
      parent: host.current ?? undefined,
    });
    view.current = editor;
    if (autoFocus) {
      editor.focus();
      editor.dispatch({ selection: { anchor: editor.state.doc.length } });
    }
    return () => {
      editor.destroy();
      view.current = null;
    };
    // Mounted once per entry — the pane is keyed on the entry id, so a new
    // entry is a new editor. Later `value` changes are pushed in below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Keeps the editor honest when the value is changed from outside (the pane
  // reloading an entry sync pulled in), without touching the document while
  // the user is typing into it. The replacement is reported through
  // `onChange` like any edit; `EntryPane` swallows that echo.
  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    const current = editor.state.doc.toString();
    if (current === value) return;
    editor.dispatch({
      changes: { from: 0, to: current.length, insert: value },
    });
  }, [value]);

  // `ThemedApp` memoises the theme per (mode, scheme), so its identity only
  // changes when the palette or font does.
  useEffect(() => {
    view.current?.dispatch({
      effects: themes.current.reconfigure(themeExtensions(theme)),
    });
  }, [theme]);

  return (
    <Box
      ref={host}
      data-testid="entry-body"
      // The editor is only as tall as its text, so clicking the space under a
      // short entry would otherwise hit nothing. Put the caret at the end,
      // which is where someone clicking below the last line means to write.
      onMouseDown={(event) => {
        if (event.target !== host.current) return;
        event.preventDefault();
        const editor = view.current;
        if (!editor) return;
        editor.focus();
        editor.dispatch({ selection: { anchor: editor.state.doc.length } });
      }}
      sx={{ minHeight: BODY_MIN_HEIGHT, cursor: "text" }}
    />
  );
}
