//! Notification-area icon on its own thread with its own message loop, so
//! it keeps working while the main window is hidden.

use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::engine::Engine;
use crate::platform::wide;
use crate::tray_badge::Badge;

const WM_TRAY: u32 = WM_APP + 1;
// The optional Command Palette extension reads this window-owned value.
const BATTERY_PROPERTY: windows_sys::core::PCWSTR = windows_sys::core::w!("DSZ.BatteryPercent.v1");

fn battery_property(percent: Option<u8>) -> HANDLE {
    // Null means the running build doesn't publish the property.
    percent.map_or(1, |value| usize::from(value.min(100)) + 2) as HANDLE
}
const ID_SHOW: usize = 1;
const ID_QUIT: usize = 2;
const ID_PROFILE_BASE: usize = 100;
// Independent from the main icon, so Explorer can distinguish their
// visibility preferences across application restarts.
const BATTERY_GUID: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x82886fea, data2: 0x0abd, data3: 0x43cc, data4: [0x9b,0x0d,0x73,0xab,0x98,0x41,0x62,0xb0],
};

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();
static ON_SHOW: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn set_main_window(h: isize) {
    MAIN_HWND.store(h, Ordering::Release);
}

pub fn main_window() -> HWND {
    MAIN_HWND.load(Ordering::Acquire) as HWND
}

pub fn show_main_window() {
    let mut h = main_window();
    if h.is_null() {
        // Started hidden and the UI hasn't found its window yet.
        let t = crate::platform::wide(crate::APP_TITLE);
        h = unsafe { FindWindowW(std::ptr::null(), t.as_ptr()) };
        if !h.is_null() {
            set_main_window(h as isize);
        }
    }
    if !h.is_null() {
        unsafe {
            ShowWindow(h, SW_SHOW);
            if IsIconic(h) != 0 {
                ShowWindow(h, SW_RESTORE);
            }
            SetForegroundWindow(h);
        }
    }
    if let Some(f) = ON_SHOW.get() {
        f();
    }
}

pub fn hide_main_window() {
    let h = main_window();
    if !h.is_null() {
        unsafe {
            ShowWindow(h, SW_HIDE);
        }
    }
}

pub use crate::icon_art::icon_rgba;

/// The tray icon at the size Windows wants: the multi-size icon embedded
/// in the exe (resource 1, see `build.rs`), else drawn here.
unsafe fn make_hicon(badge: Badge, color: [u8; 3], battery_only: bool) -> HICON {
    let cx = GetSystemMetrics(SM_CXSMICON).max(16);
    let cy = GetSystemMetrics(SM_CYSMICON).max(16);
    let h = if badge == Badge::Off || (battery_only && badge == Badge::Disconnected) { LoadImageW(
        GetModuleHandleW(null()),
        1 as _,
        IMAGE_ICON,
        cx,
        cy,
        LR_DEFAULTCOLOR,
    ) } else { null_mut() };
    if !h.is_null() {
        return h as HICON;
    }
    let size = cx as usize;
    let rgba = crate::tray_badge::render(size, badge, color, battery_only);
    // CreateIcon wants BGRA for the color plane and a 1-bpp AND mask.
    let mut bgra = vec![0u8; size * size * 4];
    for i in 0..size * size {
        bgra[i * 4] = rgba[i * 4 + 2];
        bgra[i * 4 + 1] = rgba[i * 4 + 1];
        bgra[i * 4 + 2] = rgba[i * 4];
        bgra[i * 4 + 3] = rgba[i * 4 + 3];
    }
    // Mask rows are padded to 16 bits.
    let and_mask = vec![0u8; size.div_ceil(16) * 2 * size];
    CreateIcon(
        GetModuleHandleW(null()),
        size as i32,
        size as i32,
        1,
        32,
        and_mask.as_ptr(),
        bgra.as_ptr(),
    )
}

/// A notification from the tray icon; clicking it opens the window on
/// Settings.
pub fn notify(title: &str, text: &str) {
    let hwnd = TRAY_HWND.load(Ordering::Acquire) as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        let t: Vec<u16> = title.encode_utf16().take(63).collect();
        nid.szInfoTitle[..t.len()].copy_from_slice(&t);
        let b: Vec<u16> = text.encode_utf16().take(255).collect();
        nid.szInfo[..b.len()].copy_from_slice(&b);
        Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn set_tip(nid: &mut NOTIFYICONDATAW, text: &str) {
    let w: Vec<u16> = text.encode_utf16().take(127).collect();
    nid.szTip = [0; 128];
    nid.szTip[..w.len()].copy_from_slice(&w);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let ev = (lp & 0xFFFF) as u32;
            if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                show_main_window();
            } else if ev == NIN_BALLOONUSERCLICK {
                crate::update::request_settings();
                show_main_window();
            } else if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                show_menu(hwnd);
            }
            0
        }
        WM_COMMAND => {
            let id = wp & 0xFFFF;
            if id == ID_SHOW {
                show_main_window();
            } else if id == ID_QUIT {
                quit();
            } else if id >= ID_PROFILE_BASE {
                if let Some(e) = ENGINE.get() {
                    let names = e.profiles.names();
                    if let Some(name) = names.get(id - ID_PROFILE_BASE) {
                        e.select_profile(name);
                    }
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    let show = wide(crate::i18n::tr("Open DSZ"));
    AppendMenuW(menu, MF_STRING, ID_SHOW, show.as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    let mut current = String::new();
    if let Some(e) = ENGINE.get() {
        if let Some(d) = e.device_list().first() {
            current = e.profile_name_for(d);
        } else {
            current = e.settings.read().default_profile.clone();
        }
        let sub = CreatePopupMenu();
        let mut labels = Vec::new();
        for (i, n) in e.profiles.names().iter().enumerate().take(50) {
            let w = wide(n);
            let flags = if *n == current {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            AppendMenuW(sub, flags, ID_PROFILE_BASE + i, w.as_ptr());
            labels.push(w);
        }
        let p = wide(crate::i18n::tr("Profile"));
        AppendMenuW(menu, MF_POPUP, sub as usize, p.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
    }
    let quit = wide(crate::i18n::tr("Quit"));
    AppendMenuW(menu, MF_STRING, ID_QUIT, quit.as_ptr());
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
    let _ = current;
}

pub fn quit() -> ! {
    if let Some(e) = ENGINE.get() {
        e.shutdown();
    }
    std::process::exit(0);
}

/// Spawn the tray thread. `on_show` runs after the window is shown (used to
/// wake the UI).
pub fn start(engine: Arc<Engine>, on_show: impl Fn() + Send + Sync + 'static) {
    let _ = ENGINE.set(engine.clone());
    let _ = ON_SHOW.set(Box::new(on_show));
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || unsafe {
            let class = wide("DSZTray");
            let hinst = GetModuleHandleW(null());
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: null_mut(),
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                hinst,
                null(),
            );
            if hwnd.is_null() {
                log::warn!("tray window creation failed");
                return;
            }
            TRAY_HWND.store(hwnd as isize, Ordering::Release);
            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            nid.uCallbackMessage = WM_TRAY;
            let mut main_state = (Badge::Off, [0; 3], false);
            nid.hIcon = make_hicon(main_state.0, main_state.1, main_state.2);
            set_tip(&mut nid, "DSZ");
            Shell_NotifyIconW(NIM_ADD, &nid);
            let mut battery_nid: NOTIFYICONDATAW = std::mem::zeroed();
            battery_nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            battery_nid.hWnd = hwnd;
            battery_nid.uID = 2;
            battery_nid.guidItem = BATTERY_GUID;
            battery_nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_GUID;
            battery_nid.uCallbackMessage = WM_TRAY;
            let mut battery_added = false;
            let mut battery_state = None;
            crate::cmdpal::start(hwnd, &engine);
            let mut published = (SetPropW(hwnd, BATTERY_PROPERTY, battery_property(None)) != 0).then_some(None);

            // Read cached state only: icon changes add no controller traffic.
            SetTimer(hwnd, 1, 500, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                if msg.message == WM_TIMER {
                    let devs = engine.device_list();
                    let chosen = devs.iter().min_by_key(|d| d.live.lock().input.battery_shown());
                    let name = chosen.map(|d| engine.profile_name_for(d)).unwrap_or_else(|| engine.settings.read().default_profile.clone());
                    let percent = chosen.map(|d| d.live.lock().input.battery_shown().min(100));
                    if published != Some(percent) && SetPropW(hwnd, BATTERY_PROPERTY, battery_property(percent)) != 0 {
                        published = Some(percent);
                    }
                    let style = engine.profiles.find(&name).map(|p| p.lighting.tray_badge.clone()).unwrap_or_default();
                    let plan = crate::tray_badge::display_plan(&style, percent);
                    let old_icon = if plan.main != main_state {
                        let next = make_hicon(plan.main.0, plan.main.1, plan.main.2);
                        if !next.is_null() { main_state = plan.main; std::mem::replace(&mut nid.hIcon, next) } else { null_mut() }
                    } else { null_mut() };
                    let tip = if devs.is_empty() {
                        "DSZ — no controller".to_string()
                    } else {
                        let parts: Vec<String> = devs
                            .iter()
                            .map(|d| {
                                format!(
                                    "{} {}%",
                                    d.model.name(),
                                    d.live.lock().input.battery_shown()
                                )
                            })
                            .collect();
                        format!("DSZ — {}", parts.join(", "))
                    };
                    set_tip(&mut nid, &tip);
                    Shell_NotifyIconW(NIM_MODIFY, &nid);
                    if !old_icon.is_null() { DestroyIcon(old_icon); }
                    if let Some(state) = plan.separate {
                        let old = if battery_state != Some(state) || battery_nid.hIcon.is_null() {
                            let next = make_hicon(state.0, state.1, true);
                            if !next.is_null() { battery_state = Some(state); std::mem::replace(&mut battery_nid.hIcon, next) } else { null_mut() }
                        } else { null_mut() };
                        set_tip(&mut battery_nid, &format!("DSZ battery — {}%", percent.unwrap_or(0)));
                        if !battery_nid.hIcon.is_null() {
                            let operation = if battery_added { NIM_MODIFY } else { NIM_ADD };
                            battery_added = Shell_NotifyIconW(operation, &battery_nid) != 0;
                        }
                        if !old.is_null() { DestroyIcon(old); }
                    } else {
                        if battery_added { Shell_NotifyIconW(NIM_DELETE, &battery_nid); battery_added = false; }
                        if !battery_nid.hIcon.is_null() { DestroyIcon(battery_nid.hIcon); battery_nid.hIcon = null_mut(); }
                        battery_state = None;
                    }
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            RemovePropW(hwnd, BATTERY_PROPERTY);
            Shell_NotifyIconW(NIM_DELETE, &nid);
            if battery_added { Shell_NotifyIconW(NIM_DELETE, &battery_nid); }
            if !battery_nid.hIcon.is_null() { DestroyIcon(battery_nid.hIcon); }
            if !nid.hIcon.is_null() { DestroyIcon(nid.hIcon); }
        })
        .unwrap();
}

#[cfg(test)]
mod tests {
    #[test]
    fn palette_property_distinguishes_disconnected_from_empty_battery() {
        assert_eq!(super::battery_property(None) as usize, 1);
        assert_eq!(super::battery_property(Some(0)) as usize, 2);
        assert_eq!(super::battery_property(Some(100)) as usize, 102);
        assert_eq!(super::battery_property(Some(255)) as usize, 102);
    }
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::*;

    #[test]
    fn windows_icon_keeps_colored_badge_and_opaque_background_over_white() {
        for battery_only in [false, true] {
        unsafe {
            let size = GetSystemMetrics(SM_CXSMICON).max(16) as usize;
            let icon = make_hicon(Badge::Level(80), [255, 0, 0], battery_only);
            assert!(!icon.is_null());
            let dc = CreateCompatibleDC(null_mut());
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = size as i32;
            info.bmiHeader.biHeight = -(size as i32);
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            let mut pixels = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
            assert!(!bitmap.is_null() && !pixels.is_null());
            let old = SelectObject(dc, bitmap);
            let data = std::slice::from_raw_parts_mut(pixels as *mut u8, size * size * 4);
            data.fill(255);
            let drawn = DrawIconEx(dc, 0, 0, icon, size as i32, size as i32, 0, null_mut(), DI_NORMAL);
            GdiFlush();
            let result = data.to_vec();
            SelectObject(dc, old); DeleteObject(bitmap); DeleteDC(dc); DestroyIcon(icon);
            assert_ne!(drawn, 0);
            assert!(result.chunks_exact(4).any(|p| p[..3] == [0, 0, 255]), "Windows must draw the selected red text");
            let left = size - (7 * (size / 16).max(1) + 2);
            assert_eq!(&result[left * 4..left * 4 + 3], &[0, 0, 0], "badge background must cover the white icon at the top right");
            if battery_only { assert!(result.chunks_exact(4).all(|p| p[..3] == [0,0,0] || p[..3] == [0,0,255])); }
        }
        }
    }
}
