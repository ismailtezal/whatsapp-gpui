//! Downloading the media of stored messages, and uploading files to send.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use whatsapp_rust::async_channel;
use whatsapp_rust::prelude::Client;

use crate::backend::{notify, quote_context, send_prepared};
use crate::events::{Attach, Command, UiEvent};
use crate::store::Store;

/// At most this many downloads run at once.
fn download_slots() -> &'static tokio::sync::Semaphore {
    static SLOTS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| tokio::sync::Semaphore::new(4))
}

fn extension_for(mime: Option<&str>, fallback: &str) -> String {
    let mime = mime.unwrap_or("").split(';').next().unwrap_or("").trim();
    let known = match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/3gpp" => "3gp",
        "video/quicktime" => "mov",
        "audio/ogg" => "ogg",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/aac" => "aac",
        "audio/wav" => "wav",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "text/plain" => "txt",
        "application/was" => "was",
        _ => "",
    };
    if known.is_empty() {
        fallback.to_string()
    } else {
        known.to_string()
    }
}

/// The MIME type to declare for a file being sent, from its extension.
pub fn mime_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "3gp" => "video/3gpp",
        "ogg" | "opus" => "audio/ogg; codecs=opus",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" => "text/plain",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

/// Which kind of attachment a file is sent as when the sender did not choose.
pub fn attach_kind_for(path: &Path) -> Attach {
    let mime = mime_for(path);
    if mime == "image/gif" {
        Attach::Document
    } else if mime.starts_with("image/") {
        Attach::Image
    } else if mime.starts_with("video/") {
        Attach::Video
    } else if mime.starts_with("audio/") {
        Attach::Audio
    } else {
        Attach::Document
    }
}

fn safe_name(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn safe_file_name(name: &str) -> String {
    const FORBIDDEN: &str = "<>:\"/\\|?*";
    name.chars()
        .map(|c| {
            if FORBIDDEN.contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// The outcome of fetching one message's media: the bytes (or why not), the file
/// extension, and the original file name for documents.
type Fetched = (std::result::Result<Vec<u8>, String>, String, Option<String>);

/// Downloads whichever media the message carries. `None` when it carries none.
async fn fetch(client: &Arc<Client>, base: &crate::wa::Message) -> Option<Fetched> {
    use whatsapp_rust::download::DownloadLimits;
    let limits = DownloadLimits::default();
    fn text<E: std::fmt::Display>(outcome: std::result::Result<Vec<u8>, E>) -> std::result::Result<Vec<u8>, String> {
        outcome.map_err(|error| error.to_string())
    }
    if let Some(m) = base.image_message.as_option() {
        return Some((
            text(client.download(m, limits).await),
            extension_for(m.mimetype.as_deref(), "jpg"),
            None,
        ));
    }
    if let Some(m) = base.sticker_message.as_option() {
        return Some((
            text(client.download(m, limits).await),
            extension_for(m.mimetype.as_deref(), "webp"),
            None,
        ));
    }
    if let Some(m) = base.video_message.as_option().or(base.ptv_message.as_option()) {
        return Some((
            text(client.download(m, limits).await),
            extension_for(m.mimetype.as_deref(), "mp4"),
            None,
        ));
    }
    if let Some(m) = base.audio_message.as_option() {
        return Some((
            text(client.download(m, limits).await),
            extension_for(m.mimetype.as_deref(), "ogg"),
            None,
        ));
    }
    if let Some(m) = base.document_message.as_option() {
        let name = m.file_name.clone().or_else(|| m.title.clone());
        let from_name = name
            .as_deref()
            .and_then(|n| Path::new(n).extension())
            .and_then(|e| e.to_str())
            .map(str::to_lowercase);
        let extension = from_name.unwrap_or_else(|| extension_for(m.mimetype.as_deref(), "bin"));
        return Some((text(client.download(m, limits).await), extension, name));
    }
    None
}

/// The 32-byte media key of whichever media the message carries.
fn media_key(base: &crate::wa::Message) -> Option<[u8; 32]> {
    let key = base
        .image_message
        .as_option()
        .and_then(|m| m.media_key.clone())
        .or_else(|| base.video_message.as_option().and_then(|m| m.media_key.clone()))
        .or_else(|| base.ptv_message.as_option().and_then(|m| m.media_key.clone()))
        .or_else(|| base.audio_message.as_option().and_then(|m| m.media_key.clone()))
        .or_else(|| base.document_message.as_option().and_then(|m| m.media_key.clone()))
        .or_else(|| base.sticker_message.as_option().and_then(|m| m.media_key.clone()))?;
    key.as_slice().try_into().ok()
}

/// Points the message's media at a fresh download path.
fn set_direct_path(base: &mut crate::wa::Message, path: &str) {
    let path = Some(path.to_string());
    if let Some(m) = base.image_message.as_option_mut() {
        m.direct_path = path;
    } else if let Some(m) = base.video_message.as_option_mut() {
        m.direct_path = path;
    } else if let Some(m) = base.ptv_message.as_option_mut() {
        m.direct_path = path;
    } else if let Some(m) = base.audio_message.as_option_mut() {
        m.direct_path = path;
    } else if let Some(m) = base.document_message.as_option_mut() {
        m.direct_path = path;
    } else if let Some(m) = base.sticker_message.as_option_mut() {
        m.direct_path = path;
    }
}

/// Whether a download failure means the server no longer has the file under this path.
fn expired(error: &str) -> bool {
    ["Some(403)", "Some(404)", "Some(410)"].iter().any(|code| error.contains(code))
}

/// Asks the phone to upload the media again and returns the message pointed at the new copy.
async fn reupload(
    client: &Arc<Client>,
    record: &crate::wa::WebMessageInfo,
    chat: &str,
    id: &str,
    base: &crate::wa::Message,
) -> Result<crate::wa::Message> {
    use whatsapp_rust::{MediaReuploadRequest, MediaRetryResult, MessageId, MessageRef};
    let key = media_key(base).ok_or_else(|| anyhow!("the message has no media key"))?;
    let (chat_jid, sender, from_me) = crate::backend::target_parts(record, chat)?;
    let target = MessageRef::new(&chat_jid, MessageId::new(id)?, sender.as_ref(), from_me)?;
    let request = MediaReuploadRequest {
        target,
        media_key: &key,
    };
    match client
        .media_reupload()
        .request(&request)
        .await
        .map_err(|error| anyhow!("asking the phone for the file failed: {error}"))?
    {
        MediaRetryResult::Success { direct_path } => {
            let mut fresh = base.clone();
            set_direct_path(&mut fresh, &direct_path.to_string());
            Ok(fresh)
        }
        other => Err(anyhow!("the phone no longer has this file ({other:?})")),
    }
}

/// Downloads and decrypts the media of a stored message into the media folder. When the
/// server copy has expired, the phone is asked to upload it again first.
async fn download_media(
    client: &Arc<Client>,
    store: &Arc<Store>,
    events: &async_channel::Sender<UiEvent>,
    chat: &str,
    id: &str,
) -> Result<()> {
    let record = store
        .record(chat, id)?
        .ok_or_else(|| anyhow!("message not found"))?;
    let message = record
        .message
        .as_option()
        .ok_or_else(|| anyhow!("message has no content"))?;
    let (base, _, _) = crate::content::unwrap(message);
    let unwrapped = std::ptr::eq(base, message);

    let slot = download_slots().acquire().await?;
    let mut fetched = fetch(client, base)
        .await
        .ok_or_else(|| anyhow!("this message has no downloadable media"))?;
    if let Err(error) = &fetched.0
        && expired(error)
    {
        match reupload(client, &record, chat, id, base).await {
            Ok(fresh) => {
                if let Some(retried) = fetch(client, &fresh).await {
                    fetched = retried;
                }
                // Remember the new path, so the next download does not need the phone.
                if fetched.0.is_ok() && unwrapped {
                    let mut updated = record.clone();
                    updated.message = whatsapp_rust::buffa::MessageField::some(fresh);
                    store.put_message(&updated)?;
                }
            }
            Err(error) => fetched.0 = Err(format!("{error:#}")),
        }
    }
    drop(slot);
    let (bytes, extension, file_name) = fetched;

    let (path, error) = match bytes {
        Ok(bytes) => {
            // Documents keep their own name inside a per-message folder; the rest are
            // named after the message.
            let path = match file_name {
                Some(name) => {
                    let dir = crate::paths::media_dir().join(safe_name(id));
                    std::fs::create_dir_all(&dir)?;
                    let clean = safe_file_name(&name);
                    dir.join(if clean.trim().is_empty() {
                        format!("document.{extension}")
                    } else {
                        clean
                    })
                }
                None => crate::paths::media_dir().join(format!("{}.{extension}", safe_name(id))),
            };
            std::fs::write(&path, &bytes)?;
            // Videos get a preview picture, made by Windows, so they do not show as blanks.
            if matches!(extension.as_str(), "mp4" | "mov" | "3gp" | "m4v") {
                let video = path.clone();
                match tokio::task::spawn_blocking(move || crate::thumbs::make_video_thumb(&video)).await {
                    Ok(Err(error)) => eprintln!("video preview failed: {error:#}"),
                    Err(error) => eprintln!("video preview task failed: {error}"),
                    Ok(Ok(_)) => {}
                }
            }
            let path = path.to_string_lossy().to_string();
            store.batch(|tx| tx.set_media_path(chat, id, &path))?;
            (Some(path), None)
        }
        Err(error) => (None, Some(error)),
    };
    let failed = error.clone();
    let _ = events
        .send(UiEvent::MediaReady {
            chat: chat.to_string(),
            id: id.to_string(),
            path,
            error,
        })
        .await;
    let _ = events
        .send(UiEvent::ChatChanged {
            chat: chat.to_string(),
        })
        .await;
    match failed {
        Some(error) => Err(anyhow!("download failed: {error}")),
        None => Ok(()),
    }
}

/// A small JPEG preview and the pixel size of a picture, for the message's thumbnail.
fn picture_preview(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let picture = image::load_from_memory(data).ok()?;
    let (width, height) = (picture.width(), picture.height());
    let thumb = picture.thumbnail(100, 100).to_rgb8();
    let mut out = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 60);
    encoder
        .encode(
            &thumb,
            thumb.width(),
            thumb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some((width, height, out))
}

/// Uploads a file and sends it as the given kind of attachment. Returns the message ID.
#[allow(clippy::too_many_arguments)]
pub async fn send_file(
    client: &Arc<Client>,
    store: &Arc<Store>,
    events: &async_channel::Sender<UiEvent>,
    chat: &str,
    path: &Path,
    kind: Attach,
    caption: Option<String>,
    reply_to: Option<String>,
) -> Result<String> {
    let message = build_file_message(client, store, chat, path, kind, caption, reply_to).await?;
    let id = send_prepared(client, store, events, chat, message).await?;
    // Our own copy is already on disk: point the message at it so it shows without a download.
    let local = path.to_string_lossy().to_string();
    store.batch(|tx| tx.set_media_path(chat, &id, &local))?;
    notify(events, chat).await;
    Ok(id)
}

/// Uploads a file and builds the message that carries it, without sending it.
pub(crate) async fn build_file_message(
    client: &Arc<Client>,
    store: &Arc<Store>,
    chat: &str,
    path: &Path,
    kind: Attach,
    caption: Option<String>,
    reply_to: Option<String>,
) -> Result<crate::wa::Message> {
    use whatsapp_rust::media::{self, AudioOptions, DocumentOptions, ImageOptions, VideoOptions};
    use whatsapp_rust::wacore::download::MediaType;

    let data = tokio::fs::read(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let mime = mime_for(path).to_string();
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let caption = caption.filter(|c| !c.trim().is_empty());
    let context = match reply_to {
        Some(id) => store
            .record(chat, &id)?
            .and_then(|r| quote_context(&r, chat))
            .map(Box::new),
        None => None,
    };

    let message = match kind {
        Attach::Image => {
            let preview_data = data.clone();
            let preview = tokio::task::spawn_blocking(move || picture_preview(&preview_data)).await?;
            let upload = client
                .upload(data, MediaType::Image, Default::default())
                .await?;
            let mut options = ImageOptions::default();
            options.caption = caption;
            options.mimetype = Some(mime);
            options.jpeg_thumbnail = preview.as_ref().map(|p| p.2.clone());
            options.context_info = context;
            let mut message = media::image_message(upload, options);
            if let (Some((width, height, _)), Some(image)) =
                (preview, message.image_message.as_option_mut())
            {
                image.width = Some(width);
                image.height = Some(height);
            }
            message
        }
        Attach::Video => {
            let upload = client
                .upload(data, MediaType::Video, Default::default())
                .await?;
            let mut options = VideoOptions::default();
            options.caption = caption;
            options.mimetype = Some(mime);
            options.context_info = context;
            media::video_message(upload, options)
        }
        Attach::Audio | Attach::Voice => {
            // A voice message carries its length and the bars drawn on it; both were
            // saved beside the recording when it was made.
            let voice = (kind == Attach::Voice)
                .then(|| crate::voice::saved_details(path))
                .flatten();
            let upload = client
                .upload(data, MediaType::Audio, Default::default())
                .await?;
            let mut options = AudioOptions::default();
            options.mimetype = Some(mime);
            options.ptt = Some(kind == Attach::Voice);
            if let Some((seconds, waveform)) = voice {
                options.duration_seconds = Some(seconds);
                options.waveform = Some(waveform);
            }
            options.context_info = context;
            media::audio_message(upload, options)
        }
        Attach::Sticker => {
            // A sticker is a WebP sent under its own media type, with its pixel size.
            let size_data = data.clone();
            let size = tokio::task::spawn_blocking(move || {
                image::load_from_memory(&size_data)
                    .ok()
                    .map(|picture| (picture.width(), picture.height()))
            })
            .await?;
            let upload = client
                .upload(data, MediaType::Sticker, Default::default())
                .await?;
            let mut sticker = crate::wa::message::StickerMessage::default();
            sticker.url = Some(upload.url.clone());
            sticker.direct_path = Some(upload.direct_path.clone());
            sticker.media_key = Some(upload.media_key.to_vec());
            sticker.file_sha256 = Some(upload.file_sha256.to_vec());
            sticker.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
            sticker.file_length = Some(upload.file_length);
            sticker.media_key_timestamp = Some(upload.media_key_timestamp);
            sticker.mimetype = Some("image/webp".to_string());
            if let Some((width, height)) = size {
                sticker.width = Some(width);
                sticker.height = Some(height);
            }
            if let Some(context) = context {
                sticker.context_info = whatsapp_rust::buffa::MessageField::from_box(context);
            }
            let mut message = crate::wa::Message::default();
            message.sticker_message = whatsapp_rust::buffa::MessageField::some(sticker);
            message
        }
        Attach::Document => {
            let upload = client
                .upload(data, MediaType::Document, Default::default())
                .await?;
            let mut options = DocumentOptions::default();
            options.mimetype = Some(mime);
            options.file_name = Some(file_name.clone());
            options.title = Some(file_name);
            options.caption = caption;
            options.context_info = context;
            media::document_message(upload, options)
        }
    };

    Ok(message)
}

/// Runs the commands that deal with files.
pub(crate) async fn execute(
    client: &Arc<Client>,
    store: &Arc<Store>,
    events: &async_channel::Sender<UiEvent>,
    command: Command,
) -> Result<()> {
    match command {
        Command::DownloadMedia { chat, id } => download_media(client, store, events, &chat, &id).await,
        Command::SendFile {
            chat,
            path,
            kind,
            caption,
            reply_to,
        } => send_file(client, store, events, &chat, &path, kind, caption, reply_to)
            .await
            .map(|_| ()),
        other => Err(anyhow!("not a media command: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_attachment_kind_from_extension() {
        assert_eq!(attach_kind_for(Path::new("a.JPG")), Attach::Image);
        assert_eq!(attach_kind_for(Path::new("clip.mp4")), Attach::Video);
        assert_eq!(attach_kind_for(Path::new("song.mp3")), Attach::Audio);
        assert_eq!(attach_kind_for(Path::new("report.pdf")), Attach::Document);
        assert_eq!(attach_kind_for(Path::new("unknown.xyz")), Attach::Document);
    }

    #[test]
    fn file_names_lose_characters_windows_forbids() {
        assert_eq!(safe_file_name("a:b?c.txt"), "a_b_c.txt");
        assert_eq!(safe_name("3EB0/AB+=="), "3EB0_AB___");
    }

    #[test]
    fn extensions_follow_mime_types() {
        assert_eq!(extension_for(Some("image/jpeg"), "bin"), "jpg");
        assert_eq!(extension_for(Some("audio/ogg; codecs=opus"), "bin"), "ogg");
        assert_eq!(extension_for(None, "bin"), "bin");
    }
}
