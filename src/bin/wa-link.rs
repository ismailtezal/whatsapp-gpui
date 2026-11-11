//! Step 1: link this client to a WhatsApp account by QR code.
//!
//! The session is stored in %LOCALAPPDATA%\whatsapp-gpui\session.db, so the
//! account stays linked across restarts and the GUI can reuse it later.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use qrcode::QrCode;
use qrcode::render::svg;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::store::DevicePropsOverride;
use whatsapp_rust::wacore::store::device::default_history_sync_config;
use whatsapp_rust::wacore::types::events::Event;
use whatsapp_rust::waproto::whatsapp::device_props::{HistorySyncConfig, PlatformType};

static PAGE_OPENED: AtomicBool = AtomicBool::new(false);

fn data_dir() -> PathBuf {
    whatsapp_gpui::paths::data_dir()
}

fn write_page(body: &str, refresh: bool) -> PathBuf {
    let path = data_dir().join("link.html");
    let refresh_tag = if refresh {
        r#"<meta http-equiv="refresh" content="2">"#
    } else {
        ""
    };
    let html = format!(
        r#"<!doctype html><html><head><meta charset="utf-8">{refresh_tag}
<title>Link WhatsApp</title>
<style>body{{font-family:Segoe UI,sans-serif;background:#111b21;color:#e9edef;
display:flex;flex-direction:column;align-items:center;justify-content:center;
height:100vh;margin:0}}.qr{{background:#fff;padding:16px;border-radius:12px}}
p{{max-width:420px;text-align:center;line-height:1.5}}</style></head>
<body>{body}</body></html>"#
    );
    let _ = std::fs::write(&path, html);
    path
}

fn show_qr(code: &str, valid_secs: u64) {
    let Ok(qr) = QrCode::new(code.as_bytes()) else {
        eprintln!("could not encode QR code");
        return;
    };
    let image = qr
        .render::<svg::Color>()
        .min_dimensions(360, 360)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    let body = format!(
        r#"<h2>Link this client</h2><div class="qr">{image}</div>
<p>On your phone: WhatsApp &rarr; Settings &rarr; Linked devices &rarr; Link a device,
then scan this code. It refreshes by itself (valid {valid_secs}s).</p>"#
    );
    let path = write_page(&body, true);
    println!("QR code ready (valid {valid_secs}s): {}", path.display());
    if !PAGE_OPENED.swap(true, Ordering::SeqCst) {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(&path)
            .spawn();
    }
}

/// Keeps everything the server sends so nothing is lost before the GUI and
/// its history database exist: history-sync blobs are saved as-is (zlib) and
/// every event is appended to events.jsonl.
fn record_event(event: &Event) {
    use std::io::Write;
    let dir = data_dir();
    if let Event::HistorySync(sync) = event {
        let hist = dir.join("history");
        let _ = std::fs::create_dir_all(&hist);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let name = format!(
            "{stamp}-type{}-chunk{}.zlib",
            sync.sync_type(),
            sync.chunk_order().unwrap_or(0)
        );
        let _ = std::fs::write(hist.join(&name), &sync.compressed_bytes()[..]);
        println!(
            "HISTORY SYNC saved {name}: progress {:?}, {} bytes inflated",
            sync.progress(),
            sync.decompressed_size()
        );
    }
    if let Ok(line) = whatsapp_rust::serde_json::to_string(event)
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("events.jsonl"))
    {
        let _ = writeln!(file, "{line}");
    }
}

fn full_history_config() -> HistorySyncConfig {
    let mut config = default_history_sync_config();
    config.full_sync_days_limit = Some(365);
    config.on_demand_ready = Some(true);
    config.complete_on_demand_ready = Some(true);
    config
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = data_dir().join("session.db");
    println!("Session store: {}", db.display());

    let bot = Bot::builder()
        .with_backend(SqliteStore::open(&db.to_string_lossy()).await?)
        // Same pairing shape as the Windows desktop client, so the phone sends
        // the full chat history (up to a year) instead of only recent messages.
        .with_device_props(
            DevicePropsOverride::new()
                .with_os("Windows")
                .with_platform_type(PlatformType::UWP)
                .with_require_full_sync(true)
                .with_history_sync_config(full_history_config()),
        )
        .on_qr_code(|code, timeout| async move {
            show_qr(&code, timeout.as_secs());
        })
        .on_connected(|_client| async {
            println!("CONNECTED: account linked and online");
            write_page(
                "<h2>Linked &#10004;</h2><p>This client is now linked to your account. \
                 You can close this tab.</p>",
                false,
            );
        })
        .on_logged_out(|_info| async {
            println!("LOGGED OUT");
        })
        .on_message(|ctx| async move {
            println!(
                "MESSAGE from {}: {:?}",
                ctx.info.source.sender,
                ctx.message.text_content()
            );
        })
        .on_event(|event, _client| async move {
            record_event(&event);
        })
        .build()
        .await?;

    let reason = bot.run().await;
    println!("Session ended: {reason:?}");
    Ok(())
}
