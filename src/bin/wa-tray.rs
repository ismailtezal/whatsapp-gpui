//! Development check: can this machine show the tray icon? Adds it for two seconds
//! (with an unread count of 5 in its tooltip), then removes it. There is no main window
//! in this check, so clicking the icon does nothing and no taskbar badge is drawn.
//! Usage: wa-tray

fn main() {
    match whatsapp_gpui::tray::Tray::start(0) {
        Some(tray) => {
            tray.set_unread(5);
            std::thread::sleep(std::time::Duration::from_secs(2));
            tray.remove();
            println!("tray icon added, updated and removed");
        }
        None => println!("the tray icon could not be added"),
    }
    println!("start with Windows is currently: {}", whatsapp_gpui::tray::starts_with_windows());
}
