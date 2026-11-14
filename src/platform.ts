/// Whether the app runs on a phone or tablet (iOS, Android) rather than a
/// desktop. Read once from the webview: Tauri's mobile webviews report
/// `iPhone`/`iPad`/`Android` in the user agent, and iPadOS in desktop mode
/// reports `Macintosh` but is the only "Mac" with a touch screen. Used to hide
/// UI that only makes sense with a physical keyboard (the shortcuts dialog).
export const isMobile: boolean = (() => {
  if (typeof navigator !== "object") return false;
  if (/iPhone|iPad|iPod|Android/i.test(navigator.userAgent)) return true;
  return /Mac/.test(navigator.platform) && navigator.maxTouchPoints > 1;
})();
