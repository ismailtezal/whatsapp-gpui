//! Development check for the database guard: opens the chat database the way the app
//! does (repairing it if it is damaged), then runs the background upkeep (integrity
//! check and today's backup). Point it at a scratch folder with WA_DATA_DIR.
//! Usage: wa-guard

fn main() -> anyhow::Result<()> {
    println!("data folder: {}", whatsapp_gpui::paths::data_dir().display());
    if whatsapp_gpui::paths::is_private_copy() {
        println!("this is a packaged program's private copy of the folder, not the one the app uses when started from Explorer");
    }
    if std::env::args().any(|arg| arg == "--where") {
        return Ok(());
    }
    let opened = whatsapp_gpui::guard::open()?;
    println!("opened; notice: {}", opened.notice.as_deref().unwrap_or("none"));
    println!("chats: {}", opened.store.chats(true)?.len() + opened.store.chats(false)?.len());
    drop(opened);
    match whatsapp_gpui::guard::maintain() {
        Some(notice) => println!("upkeep notice: {notice}"),
        None => println!("upkeep: intact, backup made or already there"),
    }
    Ok(())
}
