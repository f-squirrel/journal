/// Generates a v4 UUID for a new entry.
///
/// `crypto.randomUUID` is secure-context only, and the iOS/macOS Tauri origin
/// (`tauri://localhost`) is a custom scheme WebKit does not treat as one, so
/// there the API is missing entirely. `crypto.getRandomValues` is *not* gated
/// that way, which is what makes the fallback below work.
///
/// `Id` is a `Uuid` on the Rust side and `save_entry` deserializes it, so the
/// fallback has to stay a real v4 UUID rather than any random string.
export function newEntryId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
