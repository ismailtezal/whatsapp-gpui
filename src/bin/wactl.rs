//! Command-line inspection of the local chat database, for development and testing.
//!
//! Chat titles and message text are masked unless `--plain` is given, so routine checks
//! do not spill private content into logs.

use anyhow::Result;
use whatsapp_gpui::content::preview_label;
use whatsapp_gpui::import::import_saved_history;
use whatsapp_gpui::store::Store;

fn mask(text: &str, plain: bool) -> String {
    if plain {
        return text.to_string();
    }
    let count = text.chars().count();
    let head: String = text.chars().take(2).collect();
    format!("{head}..({count})")
}

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let plain = args.iter().any(|a| a == "--plain");
    args.retain(|a| a != "--plain");
    let store = Store::open_default()?;
    match args.first().map(String::as_str) {
        Some("import") => {
            let started = std::time::Instant::now();
            let stats = import_saved_history(&store)?;
            println!("{stats:#?}");
            println!("took {:?}", started.elapsed());
        }
        Some("stats") => {
            let chats = store.chats(false)?;
            let archived = store.chats(true)?;
            println!("chats: {} (+{} archived)", chats.len(), archived.len());
            println!("messages: {}", store.message_count(None)?);
            println!("full-text search: {}", store.has_full_text_search());
            let mut kinds = std::collections::BTreeMap::new();
            for chat in chats.iter().chain(archived.iter()) {
                *kinds.entry(format!("{:?}", chat.kind)).or_insert(0) += 1;
            }
            println!("chat kinds: {kinds:?}");
        }
        Some("chats") => {
            let limit: usize = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(20);
            for chat in store.chats(false)?.into_iter().take(limit) {
                let preview = chat
                    .preview
                    .as_ref()
                    .map(|p| preview_label(p.kind, p.text.as_deref()))
                    .unwrap_or_default();
                println!(
                    "{:<26} {:?} unread={} pinned={} ts={} | {}",
                    mask(&chat.title, plain),
                    chat.kind,
                    chat.unread,
                    chat.pinned > 0,
                    chat.last_ts,
                    mask(&preview, plain),
                );
            }
        }
        Some("find") => {
            let needle = args.get(1).cloned().unwrap_or_default();
            for chat in store.find_chats(&needle)? {
                println!(
                    "{} | {:?} | {} | messages={}",
                    chat.jid,
                    chat.kind,
                    chat.title,
                    store.message_count(Some(&chat.jid))?
                );
            }
        }
        Some("msgs") => {
            let jid = args.get(1).cloned().unwrap_or_default();
            let limit: usize = args.get(2).and_then(|n| n.parse().ok()).unwrap_or(20);
            for msg in store.messages(&jid, None, limit)? {
                println!(
                    "{} {} {:?} {:?} {}",
                    msg.ts,
                    if msg.from_me { "me  " } else { "them" },
                    msg.kind,
                    msg.ack,
                    mask(&preview_label(msg.kind, msg.text.as_deref()), plain),
                );
            }
        }
        Some("kinds") => {
            let jid = args.get(1).cloned();
            let mut kinds = std::collections::BTreeMap::new();
            let chats = match jid {
                Some(jid) => vec![jid],
                None => store
                    .chats(false)?
                    .into_iter()
                    .chain(store.chats(true)?)
                    .map(|c| c.jid)
                    .collect(),
            };
            for chat in chats {
                for msg in store.messages(&chat, None, 100_000)? {
                    *kinds.entry(format!("{:?}", msg.kind)).or_insert(0usize) += 1;
                }
            }
            println!("{kinds:#?}");
        }
        _ => {
            eprintln!(
                "usage: wactl [--plain] import | stats | chats [n] | find <text> | msgs <jid> [n] | kinds [jid]"
            );
        }
    }
    Ok(())
}
