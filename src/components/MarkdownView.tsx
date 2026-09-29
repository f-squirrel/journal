import {
  createContext,
  useContext,
  useState,
  type ComponentProps,
} from "react";
import { Box } from "@mui/material";
import { openUrl } from "@tauri-apps/plugin-opener";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import { backend } from "../api/backend";
import { THUMB_CHAIN, useMediaUrl } from "../hooks/useMediaUrl";
import { rem } from "./chrome";
import ImageViewer from "./ImageViewer";

// The one markdown renderer: an entry's read view and the history preview.
//
// Everything passed to `ReactMarkdown` below is a module-scope constant, on
// purpose. An inline `components` object is a new set of component types on
// every render of the parent, and React remounts an element whose type
// changed: `MediaImage` would then revoke its object URL, ask the backend to
// decrypt the image again and flash, on every keystroke-driven re-render.

const REMARK_PLUGINS = [remarkGfm];

/// True below an `a`: an image there is part of the link, so it has no
/// viewer and a click on it opens the link.
const InLinkContext = createContext(false);

/// Muted box shared by the loading, unavailable and hint states. A `span`,
/// never a `div`: react-markdown puts an image inside a `<p>`.
const boxSx = {
  display: "inline-block",
  maxWidth: "100%",
  px: rem(12),
  py: rem(8),
  borderRadius: 1,
  bgcolor: "action.hover",
  color: "text.secondary",
  fontSize: rem(13),
} as const;

function MediaImage({ src, alt }: { src: string; alt: string }) {
  const { url, error } = useMediaUrl(src, THUMB_CHAIN);
  // The viewer mounts only while open, so the full image is decrypted on
  // open and its object URL revoked on close.
  const [open, setOpen] = useState(false);
  const inLink = useContext(InLinkContext);
  // The URL the webview failed to decode, so a new URL starts clean.
  const [badUrl, setBadUrl] = useState<string>();
  if (error || (url && url === badUrl))
    return (
      <Box component="span" data-testid="media-unavailable" sx={boxSx}>
        Image unavailable{alt ? `: ${alt}` : ""}
      </Box>
    );
  if (!url)
    return (
      <Box
        component="span"
        data-testid="media-loading"
        sx={{ ...boxSx, width: rem(160), height: rem(96) }}
      />
    );
  const image = (
    <Box
      component="img"
      data-testid="media-image"
      src={url}
      alt={alt}
      onError={() => setBadUrl(url)}
      sx={{ maxWidth: "100%", maxHeight: rem(240), width: "auto" }}
    />
  );
  if (inLink) return image;
  // A native button: phrasing content, so it is valid inside the `<p>`.
  return (
    <>
      <Box
        component="button"
        type="button"
        data-testid="media-thumb"
        aria-label={`Open image${alt ? `: ${alt}` : ""}`}
        onClick={() => setOpen(true)}
        sx={{
          display: "inline-block",
          p: 0,
          m: 0,
          border: 0,
          bgcolor: "transparent",
          font: "inherit",
          cursor: "zoom-in",
          maxWidth: "100%",
          verticalAlign: "bottom",
        }}
      >
        {image}
      </Box>
      {open && (
        <ImageViewer
          src={src}
          alt={alt}
          previewUrl={url}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}

function Image({ src, alt }: ComponentProps<"img">) {
  const source = typeof src === "string" ? src : "";
  if (source.startsWith("media:"))
    return <MediaImage src={source} alt={alt ?? ""} />;
  if (source.startsWith("data:")) return <img src={source} alt={alt} />;
  // A typed path, `file://` or `https://` URL is never loaded: the webview
  // cannot read the user's files, the CSP blocks remote images, and either
  // would be neither encrypted nor synced.
  return (
    <Box component="span" data-testid="image-hint" sx={boxSx}>
      {alt ? `${alt} · ` : ""}Use Insert image to add pictures
    </Box>
  );
}

const COMPONENTS = {
  // The window has no address bar, so a click must never navigate it: an
  // entry's links open in the user's browser, where they can be seen.
  a: ({ href, children }: ComponentProps<"a">) => (
    <a
      href={href}
      onClick={(event) => {
        event.preventDefault();
        if (!href) return;
        if (backend) void openUrl(href);
        else window.open(href, "_blank", "noopener,noreferrer");
      }}
    >
      <InLinkContext.Provider value>{children}</InLinkContext.Provider>
    </a>
  ),
  img: Image,
};

/// `media:` survives only as an image's `src`. A `[x](media:…)` *link* is
/// stripped like any unknown protocol, so it never reaches `openUrl`.
function urlTransform(
  url: string,
  key: string,
  node: { tagName?: string },
): string {
  if (key === "src" && node.tagName === "img" && url.startsWith("media:"))
    return url;
  return defaultUrlTransform(url);
}

export default function MarkdownView({ markdown }: { markdown: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={REMARK_PLUGINS}
      components={COMPONENTS}
      urlTransform={urlTransform}
    >
      {markdown}
    </ReactMarkdown>
  );
}
