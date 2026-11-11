// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started as the voice encoder child: do that one job and leave.
    if whatsapp_gpui::voice::run_child_if_requested() {
        return;
    }
    whatsapp_gpui::ui::run();
}
