import { useState } from "react";
import CloseIcon from "@mui/icons-material/Close";
import { Box, CircularProgress, Dialog, IconButton } from "@mui/material";
import { useMediaUrl, VIEWER_CHAIN } from "../hooks/useMediaUrl";
import { rem } from "./chrome";

const imageSx = {
  display: "block",
  m: "auto",
  borderRadius: 0,
} as const;

/// The full-size view of one image: decrypts it on open (the caller mounts
/// this only while open, so the URL is revoked on close), fits it to the
/// screen, and toggles to actual size, scrollable, on a click or tap. While it
/// loads, and if this webview cannot show it after all, the thumbnail stands
/// in. A MUI `Dialog`, so Escape closes it and global shortcuts stay quiet
/// while it is open (ADR 0015).
export default function ImageViewer({
  src,
  alt,
  previewUrl,
  onClose,
}: {
  /// The `media:` reference.
  src: string;
  alt: string;
  /// The thumbnail's object URL.
  previewUrl?: string;
  onClose: () => void;
}) {
  const { url, error } = useMediaUrl(src, VIEWER_CHAIN);
  const [undecodable, setUndecodable] = useState(false);
  const [zoom, setZoom] = useState<"fit" | "actual">("fit");

  const full = url && !undecodable ? url : undefined;
  const failed = Boolean(error) || undecodable;
  const loading = !full && !failed;
  const toggle = () => setZoom((z) => (z === "fit" ? "actual" : "fit"));

  return (
    <Dialog
      fullScreen
      open
      onClose={onClose}
      data-testid="image-viewer"
      aria-label={alt ? `Image: ${alt}` : "Image"}
      slotProps={{
        paper: { sx: { bgcolor: "common.black", color: "common.white" } },
      }}
    >
      <IconButton
        aria-label="Close image"
        onClick={onClose}
        sx={{
          position: "absolute",
          top: rem(8),
          right: rem(8),
          zIndex: 1,
          color: "common.white",
          bgcolor: "rgba(0, 0, 0, 0.5)",
          "&:hover": { bgcolor: "rgba(0, 0, 0, 0.7)" },
        }}
      >
        <CloseIcon />
      </IconButton>
      <Box
        sx={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          overflow: "auto",
        }}
      >
        {full ? (
          <Box
            component="img"
            data-testid="viewer-image"
            data-zoom={zoom}
            src={full}
            alt={alt}
            role="button"
            tabIndex={0}
            onClick={toggle}
            onKeyDown={(event: React.KeyboardEvent) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                toggle();
              }
            }}
            onError={() => setUndecodable(true)}
            sx={{
              ...imageSx,
              cursor: zoom === "fit" ? "zoom-in" : "zoom-out",
              ...(zoom === "fit"
                ? { maxWidth: "100%", maxHeight: "100%", objectFit: "contain" }
                : { maxWidth: "none", maxHeight: "none", flexShrink: 0 }),
            }}
          />
        ) : (
          previewUrl && (
            <Box
              component="img"
              data-testid="viewer-preview"
              src={previewUrl}
              alt={alt}
              sx={{
                ...imageSx,
                maxWidth: "100%",
                maxHeight: "100%",
                objectFit: "contain",
              }}
            />
          )
        )}
      </Box>
      {loading && (
        <CircularProgress
          data-testid="viewer-loading"
          sx={{
            position: "absolute",
            top: "50%",
            left: "50%",
            mt: rem(-20),
            ml: rem(-20),
            color: "common.white",
          }}
        />
      )}
      {failed && (
        <Box
          role="alert"
          data-testid="viewer-error"
          sx={{
            position: "absolute",
            left: 0,
            right: 0,
            bottom: 0,
            p: rem(16),
            textAlign: "center",
            fontSize: rem(14),
            bgcolor: "rgba(0, 0, 0, 0.7)",
          }}
        >
          Can't show this image at full size on this device.
        </Box>
      )}
    </Dialog>
  );
}
