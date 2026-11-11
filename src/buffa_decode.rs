//! Development helper: a privacy-safe one-line description of a stored message's media
//! plumbing (which fields are present and how large), never its content.

use whatsapp_rust::buffa::Message as _;

use crate::content;
use crate::wa;

pub fn describe(proto: &[u8]) -> String {
    let Ok(record) = wa::WebMessageInfo::decode_from_slice(proto) else {
        return "undecodable".to_string();
    };
    let Some(message) = record.message.as_option() else {
        return format!("stub={:?}", record.message_stub_type);
    };
    let (base, view_once, edited) = content::unwrap(message);
    let mut parts = vec![format!("view_once={view_once} edited={edited}")];
    let mut media = |name: &str,
                     thumb: usize,
                     key: bool,
                     direct: bool,
                     url: bool,
                     mime: Option<&str>,
                     size: u64| {
        parts.push(format!(
            "{name}: thumb={thumb}B key={key} direct_path={direct} url={url} mime={} size={size}",
            mime.unwrap_or("-")
        ));
    };
    if let Some(m) = base.image_message.as_option() {
        media(
            "image",
            m.jpeg_thumbnail.as_ref().map_or(0, Vec::len),
            m.media_key.is_some(),
            m.direct_path.is_some(),
            m.url.is_some(),
            m.mimetype.as_deref(),
            m.file_length.unwrap_or(0),
        );
    }
    if let Some(m) = base.video_message.as_option() {
        media(
            "video",
            m.jpeg_thumbnail.as_ref().map_or(0, Vec::len),
            m.media_key.is_some(),
            m.direct_path.is_some(),
            m.url.is_some(),
            m.mimetype.as_deref(),
            m.file_length.unwrap_or(0),
        );
    }
    if let Some(m) = base.sticker_message.as_option() {
        media(
            "sticker",
            m.png_thumbnail.as_ref().map_or(0, Vec::len),
            m.media_key.is_some(),
            m.direct_path.is_some(),
            m.url.is_some(),
            m.mimetype.as_deref(),
            m.file_length.unwrap_or(0),
        );
    }
    if let Some(m) = base.document_message.as_option() {
        media(
            "document",
            m.jpeg_thumbnail.as_ref().map_or(0, Vec::len),
            m.media_key.is_some(),
            m.direct_path.is_some(),
            m.url.is_some(),
            m.mimetype.as_deref(),
            m.file_length.unwrap_or(0),
        );
    }
    if let Some(m) = base.audio_message.as_option() {
        media(
            "audio",
            0,
            m.media_key.is_some(),
            m.direct_path.is_some(),
            m.url.is_some(),
            m.mimetype.as_deref(),
            m.file_length.unwrap_or(0),
        );
    }
    parts.join(" | ")
}

/// The names of the top-level fields a message has set (names only, never values), for
/// finding out what an "unsupported" message actually is.
pub fn set_fields(proto: &[u8]) -> Vec<String> {
    let Ok(record) = wa::WebMessageInfo::decode_from_slice(proto) else {
        return Vec::new();
    };
    let Some(message) = record.message.as_option() else {
        return Vec::new();
    };
    let (base, _, _) = content::unwrap(message);
    match whatsapp_rust::serde_json::to_value(base) {
        Ok(whatsapp_rust::serde_json::Value::Object(map)) => map
            .into_iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(key, value)| match value {
                // One level down for wrappers we do not unwrap yet.
                whatsapp_rust::serde_json::Value::Object(inner) => {
                    let inner: Vec<String> = inner
                        .into_iter()
                        .filter(|(_, v)| !v.is_null())
                        .map(|(k, _)| k)
                        .take(8)
                        .collect();
                    format!("{key}{{{}}}", inner.join(","))
                }
                _ => key,
            })
            .collect(),
        _ => Vec::new(),
    }
}
