//! Profile pictures: fetched once, kept on disk, looked up by JID.
//!
//! A picture lives at `avatars/<jid>.jpg`. A JID known to have no picture (or one we may
//! not see) gets an empty `avatars/<jid>.none` marker so it is not asked for again.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::{Result, anyhow};
use whatsapp_rust::prelude::Client;
use whatsapp_rust::{ProfilePictureLookup, ProfilePictureRequest, ProfilePictureTarget, ProfilePictureType};

use crate::backend::Backend;
use crate::events::Command;
use crate::model::ChatKind;

fn file_stem(jid: &str) -> String {
    jid.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect()
}

fn picture_path(jid: &str) -> PathBuf {
    crate::paths::avatar_dir().join(format!("{}.jpg", file_stem(jid)))
}

fn marker_path(jid: &str) -> PathBuf {
    crate::paths::avatar_dir().join(format!("{}.none", file_stem(jid)))
}

/// Asks the server for a profile picture and saves it. Runs on the connection runtime.
pub async fn fetch(client: &Client, jid: &str) -> Result<()> {
    let target_jid = crate::backend::jid(jid)?;
    let target = match ChatKind::of_jid(jid) {
        ChatKind::Group => ProfilePictureTarget::Group(&target_jid),
        ChatKind::User => ProfilePictureTarget::Contact(&target_jid),
        _ => return Err(anyhow!("no profile picture route for {jid}")),
    };
    let lookup = client
        .pictures()
        .lookup(ProfilePictureRequest::new(target, ProfilePictureType::Preview))
        .await
        .map_err(|error| anyhow!("picture lookup failed: {error}"))?;
    match lookup {
        ProfilePictureLookup::Found(picture) => {
            let url = picture.url.to_string();
            let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
                let mut response = ureq::get(&url).call()?;
                Ok(response.body_mut().read_to_vec()?)
            })
            .await??;
            std::fs::write(picture_path(jid), bytes)?;
            let _ = std::fs::remove_file(marker_path(jid));
        }
        ProfilePictureLookup::Unchanged => {}
        _ => {
            std::fs::write(marker_path(jid), b"")?;
        }
    }
    Ok(())
}

struct Cache {
    backend: Option<Backend>,
    known: HashMap<String, Option<PathBuf>>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            backend: None,
            known: HashMap::new(),
        })
    })
}

/// Lets lookups request missing pictures through the connection.
pub fn init(backend: Backend) {
    if let Ok(mut cache) = cache().lock() {
        cache.backend = Some(backend);
    }
}

/// The picture of a JID if it is on disk. The first lookup of a JID without one asks the
/// connection to fetch it; the interface is told when it arrives.
pub fn get(jid: &str) -> Option<PathBuf> {
    if ChatKind::of_jid(jid) == ChatKind::Newsletter {
        // Channel pictures come from the channel sync; here only the disk is checked,
        // once, and the answer remembered.
        let mut cache = cache().lock().ok()?;
        if let Some(known) = cache.known.get(jid) {
            return known.clone();
        }
        let found = get_any(jid);
        cache.known.insert(jid.to_string(), found.clone());
        return found;
    }
    if !matches!(ChatKind::of_jid(jid), ChatKind::User | ChatKind::Group) || jid.ends_with("@lid") {
        return None;
    }
    let mut cache = cache().lock().ok()?;
    if let Some(known) = cache.known.get(jid) {
        return known.clone();
    }
    let path = picture_path(jid);
    let found = path.exists().then_some(path);
    if found.is_none()
        && !marker_path(jid).exists()
        && let Some(backend) = &cache.backend
    {
        backend.send(Command::FetchAvatar {
            jid: jid.to_string(),
        });
    }
    cache.known.insert(jid.to_string(), found.clone());
    found
}

/// Re-checks the disk for a JID after a fetch finished.
/// The picture kept for any address, whatever its kind, without asking the connection
/// for it (channel pictures are fetched from the web by the channel sync).
pub fn get_any(jid: &str) -> Option<PathBuf> {
    let path = picture_path(jid);
    path.exists().then_some(path)
}

/// Saves a picture as the one for `jid` (used for our own photo after changing it).
pub fn store(jid: &str, bytes: &[u8]) -> Result<()> {
    std::fs::write(picture_path(jid), bytes)?;
    let _ = std::fs::remove_file(marker_path(jid));
    refresh(jid);
    Ok(())
}

/// Forgets the picture of `jid` (used after removing our own photo).
pub fn forget(jid: &str) {
    let _ = std::fs::remove_file(picture_path(jid));
    refresh(jid);
}

/// A picture file as a profile photo: the centre square, 640 pixels, JPEG.
pub fn profile_jpeg(path: &std::path::Path) -> Result<Vec<u8>> {
    let picture = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()
        .map_err(|error| anyhow!("that file is not a picture this app can read: {error}"))?;
    let side = picture.width().min(picture.height());
    let square = picture.crop_imm(
        (picture.width() - side) / 2,
        (picture.height() - side) / 2,
        side,
        side,
    );
    let sized = square.resize_exact(640, 640, image::imageops::FilterType::Lanczos3);
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 88).encode_image(&sized.to_rgb8())?;
    Ok(bytes)
}

pub fn refresh(jid: &str) {
    if let Ok(mut cache) = cache().lock() {
        let path = picture_path(jid);
        let found = path.exists().then_some(path);
        cache.known.insert(jid.to_string(), found);
    }
}
