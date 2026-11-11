//! The notification-area (tray) icon and the unread badge on the taskbar button.
//!
//! The tray icon lives on its own thread with its own small message window, so it never
//! gets in the way of the interface's event loop. A click on it brings the main window
//! back; its menu offers Open and Quit. The main thread tells it the unread count with
//! [`Tray::set_unread`], which updates the icon's tooltip and draws a count badge over
//! the taskbar button.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    ITaskbarList3, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, HICON, HWND_MESSAGE, ICONINFO, IDI_APPLICATION,
    IsIconic, LoadIconW, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, SW_HIDE, SW_RESTORE,
    SW_SHOW, SetForegroundWindow, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

/// Sent by the tray icon to its window (mouse activity over the icon).
const WM_TRAY: u32 = WM_APP + 1;
/// Sent by the main thread: the unread count changed (count in `wParam`).
const WM_UNREAD: u32 = WM_APP + 2;
/// Sent by a second copy of the app that was started while this one runs: show the window.
const WM_SHOW: u32 = WM_APP + 3;
const MENU_OPEN: usize = 1;
const MENU_QUIT: usize = 2;

/// The main window, as a raw handle, for the tray thread to show.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Set when the owner chose Quit: closing the window then really ends the app.
static QUITTING: AtomicBool = AtomicBool::new(false);
static UNREAD: AtomicU32 = AtomicU32::new(0);

/// Whether closing the window keeps the app running behind the tray icon.
static KEEP_RUNNING: AtomicBool = AtomicBool::new(true);
/// The running tray icon, once started.
static CURRENT: std::sync::OnceLock<Tray> = std::sync::OnceLock::new();

/// The running tray icon, if there is one.
pub fn current() -> Option<Tray> {
    CURRENT.get().copied()
}

pub fn keep_running() -> bool {
    KEEP_RUNNING.load(Ordering::Relaxed)
}

pub fn set_keep_running(on: bool) {
    KEEP_RUNNING.store(on, Ordering::Relaxed);
}

/// A handle for the main thread.
#[derive(Clone, Copy)]
pub struct Tray {
    window: isize,
}

/// Whether the owner asked to quit (from the tray menu). The window's close handler
/// checks this: without it, closing only hides the window.
pub fn quitting() -> bool {
    QUITTING.load(Ordering::Relaxed)
}

/// Hides the main window; the app keeps running behind the tray icon.
pub fn hide_main_window() {
    let window = MAIN_WINDOW.load(Ordering::Relaxed);
    if window != 0 {
        // SAFETY: the handle is the app's own main window.
        unsafe {
            let _ = ShowWindow(HWND(window as *mut _), SW_HIDE);
        }
    }
}

/// Brings the main window back and to the front.
pub fn show_main_window() {
    let window = MAIN_WINDOW.load(Ordering::Relaxed);
    if window == 0 {
        return;
    }
    let window = HWND(window as *mut _);
    // SAFETY: the handle is the app's own main window.
    unsafe {
        let command = if IsIconic(window).as_bool() { SW_RESTORE } else { SW_SHOW };
        let _ = ShowWindow(window, command);
        let _ = SetForegroundWindow(window);
    }
}

impl Tray {
    /// Starts the tray icon for the given main window. Returns `None` if the tray
    /// thread could not set itself up.
    pub fn start(main_window: isize) -> Option<Tray> {
        MAIN_WINDOW.store(main_window, Ordering::Relaxed);
        let (ready, started) = std::sync::mpsc::channel::<isize>();
        std::thread::Builder::new()
            .name("wa-tray".into())
            .spawn(move || run(ready))
            .ok()?;
        let window = started.recv_timeout(std::time::Duration::from_secs(5)).ok()?;
        let tray = (window != 0).then_some(Tray { window })?;
        let _ = CURRENT.set(tray);
        Some(tray)
    }

    /// Takes the icon out of the notification area. Call when the app is about to end;
    /// otherwise the icon lingers until the pointer passes over it.
    pub fn remove(&self) {
        let window = HWND(self.window as *mut _);
        // SAFETY: removing our own icon, identified by its window and number.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(window, HICON::default(), 0));
        }
    }

    /// Shows the unread count on the tray tooltip and as a badge on the taskbar button.
    pub fn set_unread(&self, count: u32) {
        if UNREAD.swap(count, Ordering::Relaxed) == count {
            return;
        }
        // SAFETY: posting a message to the tray thread's own window.
        unsafe {
            let _ = PostMessageW(Some(HWND(self.window as *mut _)), WM_UNREAD, WPARAM(count as usize), LPARAM(0));
        }
    }
}

fn tip(count: u32) -> [u16; 128] {
    let text = match count {
        0 => "WhatsApp".to_string(),
        1 => "WhatsApp: 1 unread message".to_string(),
        many => format!("WhatsApp: {many} unread messages"),
    };
    let mut out = [0u16; 128];
    for (slot, unit) in out.iter_mut().zip(text.encode_utf16().take(127)) {
        *slot = unit;
    }
    out
}

fn icon_data(window: HWND, icon: HICON, count: u32) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        szTip: tip(count),
        ..Default::default()
    }
}

/// The app's icon from the executable (resource 1), or the system default.
fn app_icon() -> HICON {
    // SAFETY: loading an icon resource; a failure falls back to the stock icon.
    unsafe {
        let module = GetModuleHandleW(None).ok();
        module
            .and_then(|module| LoadIconW(Some(module.into()), PCWSTR(1 as *const u16)).ok())
            .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
            .unwrap_or_default()
    }
}

fn run(ready: std::sync::mpsc::Sender<isize>) {
    // SAFETY: standard Win32 window and message-loop calls on this thread only.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let class_name = w!("whatsapp-gpui-tray");
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("whatsapp-gpui tray"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        );
        let Ok(window) = window else {
            let _ = ready.send(0);
            return;
        };
        let added = Shell_NotifyIconW(NIM_ADD, &icon_data(window, app_icon(), 0)).as_bool();
        let _ = ready.send(if added { window.0 as isize } else { 0 });
        if !added {
            return;
        }
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(window, HICON::default(), 0));
    }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: called by Windows on the tray thread with this window's messages.
    unsafe {
        match message {
            WM_TRAY => {
                match lparam.0 as u32 {
                    WM_LBUTTONUP => show_main_window(),
                    WM_RBUTTONUP => show_menu(window),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_UNREAD => {
                let count = wparam.0 as u32;
                let _ = Shell_NotifyIconW(NIM_MODIFY, &icon_data(window, app_icon(), count));
                set_taskbar_badge(count);
                LRESULT(0)
            }
            WM_SHOW => {
                show_main_window();
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

/// The tray icon's menu: Open and Quit.
unsafe fn show_menu(window: HWND) {
    // SAFETY: menu calls on the tray thread; the menu is destroyed before returning.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN, w!("Open WhatsApp"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit"));
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        // Needed so the menu closes when the owner clicks elsewhere.
        let _ = SetForegroundWindow(window);
        let chosen = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD, at.x, at.y, None, window, None);
        let _ = DestroyMenu(menu);
        match chosen.0 as usize {
            MENU_OPEN => show_main_window(),
            MENU_QUIT => {
                QUITTING.store(true, Ordering::Relaxed);
                let main = MAIN_WINDOW.load(Ordering::Relaxed);
                if main != 0 {
                    let _ = PostMessageW(Some(HWND(main as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            _ => {}
        }
    }
}

/// Draws the unread count over the taskbar button (or clears it at zero).
unsafe fn set_taskbar_badge(count: u32) {
    let main = MAIN_WINDOW.load(Ordering::Relaxed);
    if main == 0 {
        return;
    }
    // SAFETY: COM was initialised on this thread; the icon is destroyed after use
    // (the taskbar keeps its own copy).
    unsafe {
        let Ok(taskbar) = CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER) else {
            return;
        };
        if taskbar.HrInit().is_err() {
            return;
        }
        let main = HWND(main as *mut _);
        if count == 0 {
            let _ = taskbar.SetOverlayIcon(main, HICON::default(), PCWSTR::null());
            return;
        }
        if let Some(icon) = badge_icon(count) {
            let _ = taskbar.SetOverlayIcon(main, icon, w!("Unread messages"));
            let _ = DestroyIcon(icon);
        }
    }
}

/// Three-by-five pixel digits, one row per byte (three low bits).
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// The badge picture: a red disc with the count in white, 32 by 32, as BGRA with
/// premultiplied alpha. Counts over 99 show as 99.
pub fn badge_pixels(count: u32) -> Vec<u8> {
    const SIDE: usize = 32;
    let mut pixels = vec![0u8; SIDE * SIDE * 4];
    let centre = (SIDE as f32 - 1.) / 2.;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let distance = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt();
            // A one-pixel soft edge.
            let cover = (15.5 - distance).clamp(0., 1.);
            let at = (y * SIDE + x) * 4;
            // #e5484d, premultiplied.
            pixels[at] = (0x4d as f32 * cover) as u8;
            pixels[at + 1] = (0x48 as f32 * cover) as u8;
            pixels[at + 2] = (0xe5 as f32 * cover) as u8;
            pixels[at + 3] = (255. * cover) as u8;
        }
    }
    let text: Vec<usize> = count
        .min(99)
        .to_string()
        .bytes()
        .map(|digit| (digit - b'0') as usize)
        .collect();
    // One digit is drawn four pixels per dot, two digits three.
    let scale = if text.len() == 1 { 4 } else { 3 };
    let width = text.len() * 3 * scale + (text.len() - 1) * scale;
    let left = (SIDE - width) / 2;
    let top = (SIDE - 5 * scale) / 2;
    for (index, digit) in text.iter().enumerate() {
        for (row, bits) in DIGITS[*digit].iter().enumerate() {
            for column in 0..3 {
                if bits & (0b100 >> column) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = left + index * 4 * scale + column * scale + dx;
                        let y = top + row * scale + dy;
                        let at = (y * SIDE + x) * 4;
                        pixels[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
    }
    pixels
}

unsafe fn badge_icon(count: u32) -> Option<HICON> {
    let pixels = badge_pixels(count);
    let mask = vec![0u8; 32 * 32 / 8];
    // SAFETY: both buffers outlive the calls; the bitmaps are deleted after the icon
    // (which copies them) is made.
    unsafe {
        let color = CreateBitmap(32, 32, 1, 32, Some(pixels.as_ptr() as *const _));
        let mask = CreateBitmap(32, 32, 1, 1, Some(mask.as_ptr() as *const _));
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// Whether the app is set to start when the owner signs in to Windows.
pub fn starts_with_windows() -> bool {
    run_reg(&["query", RUN_KEY, "/v", RUN_NAME]).is_some_and(|output| output.contains(RUN_NAME))
}

/// Makes the app start (in the background) when the owner signs in, or stops that.
pub fn set_start_with_windows(on: bool) -> bool {
    if on {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let command = format!("\"{}\" --background", exe.display());
        run_reg(&["add", RUN_KEY, "/v", RUN_NAME, "/t", "REG_SZ", "/d", &command, "/f"]).is_some()
    } else {
        run_reg(&["delete", RUN_KEY, "/v", RUN_NAME, "/f"]).is_some()
    }
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "whatsapp-gpui";

/// Runs `reg.exe` without a console window. `None` when it fails.
fn run_reg(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("reg")
        .args(args)
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_badge_is_a_red_disc_with_white_digits() {
        let pixels = badge_pixels(7);
        assert_eq!(pixels.len(), 32 * 32 * 4);
        // Corners are transparent, the rim is red, and some pixels are white.
        assert_eq!(pixels[3], 0);
        let rim = (16 * 32 + 2) * 4;
        assert!(pixels[rim + 2] > 200 && pixels[rim + 3] == 255, "rim should be opaque red");
        let white = pixels.chunks(4).filter(|p| p == &[255, 255, 255, 255]).count();
        assert!(white > 40, "the digit should be drawn: {white} white pixels");
        // Two digits fit inside the disc.
        let two = badge_pixels(42);
        let far_left = (16 * 32 + 1) * 4;
        assert_ne!(&two[far_left..far_left + 4], &[255, 255, 255, 255]);
        // More than 99 is shown as 99, not something wider.
        assert_eq!(badge_pixels(250), badge_pixels(99));
    }
}

/// Makes sure only one copy of the app runs. Two copies would share one linked-device
/// connection and knock each other off it, so nothing would send.
///
/// Returns true for the first copy. A later copy asks the first to show its window
/// (it may be hidden behind the tray icon) and gets false: it must exit at once.
pub fn claim_single_instance() -> bool {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowExW;
    // SAFETY: plain Win32 calls. The mutex handle is kept for the life of the process on
    // purpose: Windows releases it when the process ends.
    unsafe {
        let mutex = CreateMutexW(None, true, w!("Local\\whatsapp-gpui-single-instance"));
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        // The handle is deliberately never closed.
        let _ = mutex;
        if !already {
            return true;
        }
        // The first copy's tray window is a message-only window, found under HWND_MESSAGE.
        if let Ok(window) = FindWindowExW(Some(HWND_MESSAGE), None, w!("whatsapp-gpui-tray"), None) {
            let _ = PostMessageW(Some(window), WM_SHOW, WPARAM(0), LPARAM(0));
        }
        false
    }
}
