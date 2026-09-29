//! Media commands: images owned by an entry, stored as encrypted blobs.

use journal_api::{Id, MediaId, MediaMeta, MediaVariant};
use journal_vault::VaultError;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::State;

use crate::blocking;
use crate::vault::{get as vault, VaultState};

fn header<'a>(request: &'a Request<'_>, name: &str) -> Result<&'a str, String> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("missing or unreadable `{name}` header"))
}

/// The bytes travel as the raw request body, so they are not JSON-encoded;
/// the ids, the variant and the metadata ride in headers (`entry-id`,
/// `media-id`, `media-variant`, `media-meta`, the last as `MediaMeta` JSON).
#[tauri::command]
pub(crate) async fn save_media(
    state: State<'_, VaultState>,
    request: Request<'_>,
) -> Result<(), String> {
    let vault = vault(&state)?;
    let entry: Id = header(&request, "entry-id")?
        .parse()
        .map_err(|_| "bad `entry-id` header".to_owned())?;
    let media: MediaId = header(&request, "media-id")?
        .parse()
        .map_err(|_| "bad `media-id` header".to_owned())?;
    let variant: MediaVariant = serde_json::from_value(serde_json::Value::String(
        header(&request, "media-variant")?.to_owned(),
    ))
    .map_err(|_| "bad `media-variant` header".to_owned())?;
    // An unknown kind or format, or a missing dimension, is not a storable
    // media item.
    let meta: MediaMeta = serde_json::from_str(header(&request, "media-meta")?)
        .map_err(|_| VaultError::MediaType.to_string())?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("the image bytes must be sent as a raw body".to_owned());
    };
    let bytes = bytes.clone();
    // Ids and sizes only, never bytes.
    log::info!(
        "saving media: entry={entry}, media={media}, variant={variant:?}, bytes={}",
        bytes.len()
    );
    blocking(move || vault.save_media(&entry, &media, variant, &meta, &bytes)).await
}

/// The body is the plaintext payload: the `MediaMeta` JSON, a newline, then
/// the raw bytes. A missing blob is an error, since a raw response cannot be
/// `null`.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn get_media(
    state: State<'_, VaultState>,
    entry_id: Id,
    media_id: MediaId,
    variant: MediaVariant,
) -> Result<Response, String> {
    let vault = vault(&state)?;
    let media = blocking(move || {
        vault
            .get_media(&entry_id, &media_id, variant)?
            .ok_or(VaultError::MediaNotFound)
    })
    .await?;
    Ok(Response::new(media.to_payload()))
}

/// Copies the media `markdown` references into `entry_id` and returns the
/// text with its links rewritten. For conflicted copies.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn adopt_media(
    state: State<'_, VaultState>,
    entry_id: Id,
    markdown: String,
) -> Result<String, String> {
    let vault = vault(&state)?;
    blocking(move || vault.adopt_media(&entry_id, &markdown)).await
}
