//! System-tray presence for desktop resident mode — the thing next to the clock. Left-click (or menu "Show") surfaces the hidden window thru the same `PhotonEvent::ShowWindow` path the second-launch handoff uses; "Exit" is THE deliberate quit affordance residency was missing (close hides, tray exits). The icon is the round orb — same circular-mask discipline as `fluor::host::icon::Icon::to_rgba_circular`, sourced from the shipped round asset so tray and taskbar can't disagree.
//!
//! Linux: StatusNotifierItem via ksni (pure Rust zbus — no GTK, cross-compiles clean). GNOME needs the AppIndicator extension to SHOW SNI items (KDE/XFCE show them natively); without it the icon simply doesn't appear and nothing else breaks — resident behaviour still works via the second-launch handoff.
//! Windows (Shell_NotifyIcon) and macOS (NSStatusItem, main-thread-bound) are the next backends; until then `spawn` logs and returns, and residency works without a tray there too.
//! A missed message rings the orb in MAGENTA, the contacts list's unread cue (Nick 2026-11-08: "so I know if I missed a message"): `set_unread` redraws the tray icon and, on macOS, the Dock tile; the taskbar button on Windows and X11 is the window's icon, swapped by the caller thru fluor.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the orb wears the unread ring; every tray backend draws from it, so a tray spawned after the edge starts in the right state.
static UNREAD: AtomicBool = AtomicBool::new(false);

/// The tray's orb: the shipped round 64-pixel RGBA, transparent corners, the orb filling the square edge to edge.
const TRAY_ORB: &[u8] = include_bytes!("../../assets/icon-64.png");
/// The taskbar's and the Dock's orb, the same art at 256: those draw it larger than a tray slot does.
pub const WINDOW_ORB: &[u8] = include_bytes!("../../assets/icon-256.png");

/// A shipped orb asset as straight RGBA rows; with `unread`, the orb shrinks inside a MAGENTA band a quarter of the radius wide, the contacts list's unread ring at icon size.
/// Composited as the list draws its avatars: the orb goes in first and the magenta disc lands under it, showing only in the band outside the orb.
pub fn orb_rgba(asset: &[u8], unread: bool) -> Option<(Vec<u8>, u32, u32)> {
    use fluor::canvas::{Canvas, Damage};
    let img = image::load_from_memory(asset).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    if !unread {
        return Some((img.into_raw(), w, h));
    }
    // The assets are square, so one side is the diameter.
    let d = w as usize;
    let big_r = d as f32 * 0.5;
    let orb_r = big_r - big_r / ((1 << 2) as f32);
    let orb_d = (orb_r * 2.) as usize;
    let rgb: Vec<u8> = img.pixels().flat_map(|p| [p.0[0], p.0[1], p.0[2]]).collect();
    let scaled = crate::ui::avatar_render::update_avatar_scaled(&rgb, d, orb_d);
    let mut px = vec![0u32; d * d];
    let mut damage = Damage::default();
    let mut canvas = Canvas::new(&mut px, d, d, &mut damage);
    crate::ui::avatar_render::draw_avatar(&mut canvas, big_r, big_r, orb_r, &scaled, orb_d, None);
    fluor::paint::draw_circle(&mut canvas, big_r, big_r, big_r, *crate::ui::theme::RING_UNREAD_COLOUR, None);
    let mut out = Vec::with_capacity(d * d * 4);
    for p in px {
        let a = p >> 24;
        // WHY/PROOF: past the ring nothing was put in, so α is 0, and taking the darkness back out of its α divides by α; those pixels are empty and stay empty.
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        // `under` stores darkness times α and keeps it at or under α, so the straight darkness stays within a byte.
        let straight = |dark: u32| dark * 255 / a;
        let v = fluor::theme::to_sdr_visible((a << 24) | (straight((p >> 16) & 0xFF) << 16) | (straight((p >> 8) & 0xFF) << 8) | straight(p & 0xFF));
        out.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8, a as u8]);
    }
    Some((out, w, h))
}

/// A missed message is waiting (or no longer is): the tray orb wears or drops the magenta ring, and on macOS so does the Dock tile.
/// Called on the edge only, from the UI thread (the macOS status item and Dock are main-thread-bound, and the UI thread is the main thread under winit).
pub fn set_unread(unread: bool) {
    UNREAD.store(unread, Ordering::Relaxed);
    #[cfg(target_os = "linux")]
    linux::refresh();
    #[cfg(target_os = "windows")]
    windows_tray::refresh();
    #[cfg(target_os = "macos")]
    macos_tray::refresh(unread);
}

#[cfg(target_os = "linux")]
mod linux {
    use fluor::host::WakeSender;
    use std::sync::Arc;

    /// The SNI service's handle: the update capability an unread edge redraws the orb thru, kept for the process (the icon lives that long in v1).
    pub static HANDLE: std::sync::OnceLock<ksni::Handle<PhotonTray>> = std::sync::OnceLock::new();

    /// Redraw the orb: ksni reads `icon_pixmap` again after an update and signals NewIcon when it changed.
    pub fn refresh() {
        let Some(handle) = HANDLE.get().cloned() else {
            return;
        };
        crate::network::http::runtime().spawn(async move {
            if handle.update(|_| {}).await.is_none() {
                crate::log("TRAY: the SNI service is gone, so the orb cannot show the unread ring");
            }
        });
    }

    pub struct PhotonTray {
        pub proxy: Arc<dyn WakeSender<crate::ui::PhotonEvent>>,
    }

    impl ksni::Tray for PhotonTray {
        fn id(&self) -> String {
            "photon-messenger".into()
        }
        fn title(&self) -> String {
            "Photon".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            // The round orb, ringed while a message waits → SNI's network-byte-order ARGB32.
            let Some((rgba, w, h)) = super::orb_rgba(super::TRAY_ORB, super::UNREAD.load(super::Ordering::Relaxed)) else {
                return Vec::new();
            };
            let mut argb = Vec::with_capacity(rgba.len());
            for px in rgba.chunks_exact(4) {
                argb.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
            }
            vec![ksni::Icon {
                width: w as i32,
                height: h as i32,
                data: argb,
            }]
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            crate::log_err!(self.proxy.send(crate::ui::PhotonEvent::ShowWindow), "TRAY: waking the UI to show the window");
        }
        fn menu(&self) -> Vec<ksni::menu::MenuItem<Self>> {
            use ksni::menu::*;
            vec![
                StandardItem {
                    label: "Show Photon".into(),
                    activate: Box::new(|t: &mut Self| {
                        crate::log_err!(t.proxy.send(crate::ui::PhotonEvent::ShowWindow), "TRAY: waking the UI to show the window");
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Exit".into(),
                    activate: Box::new(|_t: &mut Self| {
                        // Killswitch-compliant, same as the host's Close path. The flock + control socket release with the process.
                        crate::log("TRAY: exit");
                        std::process::exit(0);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }
}

/// Put the orb next to the clock. Idempotent-ish per process (call once when residency turns on; a second call would add a second icon, so callers gate). The SNI service task rides the existing tokio runtime — tray events reach the UI thread thru the wake proxy only.
#[cfg(target_os = "linux")]
pub fn spawn(proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {
    use ksni::TrayMethods;
    let tray = linux::PhotonTray { proxy };
    crate::network::http::runtime().spawn(async move {
        match tray.spawn().await {
            Ok(handle) => {
                // The handle is the update capability; an unread edge redraws the orb thru it, and the icon lives for the process in v1 (despawn-on-toggle-off comes with a handle plumb-thru).
                if linux::HANDLE.set(handle).is_err() {
                    crate::log("TRAY: an SNI handle was already kept — a second orb is parked beside the first");
                }
                crate::log("TRAY: orb parked next to the clock (SNI; GNOME needs the AppIndicator extension to show it)");
            }
            Err(e) => crate::logf!("TRAY: SNI registration failed ({}) — no status-bar host? resident mode still works via relaunch-to-surface", e),
        }
    });
}

#[cfg(target_os = "windows")]
mod windows_tray {
    use std::sync::Arc;
    use std::sync::OnceLock;

    use fluor::host::WakeSender;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_MODIFY, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DispatchMessageW, GetCursorPos, GetMessageW, RegisterClassW, RegisterWindowMessageW,
        DestroyIcon, SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON, ICONINFO, MF_STRING,
        MSG, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE,
        WINDOW_STYLE, WM_APP, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
    };

    /// The tray thread's wake proxy — a process singleton because the WndProc has no instance pointer worth threading for one icon.
    static PROXY: OnceLock<Arc<dyn WakeSender<crate::ui::PhotonEvent>>> = OnceLock::new();
    /// Explorer's "TaskbarCreated" broadcast id — a shell restart destroys every tray icon, and re-adding on this message is how an icon survives it.
    static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
    /// The hidden tray window as an address (HWND is not Send), kept so an unread edge can swap the icon from the UI thread; Shell_NotifyIcon takes any thread.
    static TRAY_HWND: OnceLock<usize> = OnceLock::new();

    /// Redraw the orb in place: NIM_MODIFY with the icon drawn for the current unread state.
    pub fn refresh() {
        let Some(&raw) = TRAY_HWND.get() else {
            return;
        };
        unsafe {
            let Some(icon) = orb_icon() else {
                crate::log("TRAY: the orb did not draw, so the unread ring is not shown");
                return;
            };
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: HWND(raw as *mut _),
                uID: 1,
                uFlags: NIF_ICON,
                hIcon: icon,
                ..Default::default()
            };
            crate::log_err!(Shell_NotifyIconW(NIM_MODIFY, &nid).ok(), "TRAY: Shell_NotifyIcon icon swap");
            // The shell keeps its own copy of the icon.
            crate::log_err!(DestroyIcon(icon), "TRAY: icon release after the swap");
        }
    }

    const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
    const MENU_SHOW: usize = 1;
    const MENU_EXIT: usize = 2;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Build an HICON from the shipped round orb RGBA — CreateIconIndirect over a 32bpp BGRA colour bitmap plus an unused-but-required mask.
    unsafe fn orb_icon() -> Option<HICON> {
        let (rgba, w, h) = super::orb_rgba(super::TRAY_ORB, super::UNREAD.load(super::Ordering::Relaxed))?;
        let (w, h) = (w as i32, h as i32);
        let mut bgra = Vec::with_capacity(rgba.len());
        for px in rgba.chunks_exact(4) {
            bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        let colour = CreateBitmap(w, h, 1, 32, Some(bgra.as_ptr() as *const _));
        let mask = CreateBitmap(w, h, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: colour,
        };
        let icon = CreateIconIndirect(&info).ok();
        // CreateIconIndirect copied both bitmaps; a failed release leaks a GDI object.
        crate::log_err!(DeleteObject(colour).ok(), "TRAY: colour bitmap release");
        crate::log_err!(DeleteObject(mask).ok(), "TRAY: mask bitmap release");
        icon
    }

    unsafe fn add_icon(hwnd: HWND) {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY_CALLBACK,
            ..Default::default()
        };
        if let Some(icon) = orb_icon() {
            nid.hIcon = icon;
        }
        let tip = wide("Photon");
        nid.szTip[..tip.len().min(128)].copy_from_slice(&tip[..tip.len().min(128)]);
        crate::log_err!(Shell_NotifyIconW(NIM_ADD, &nid).ok(), "TRAY: Shell_NotifyIcon add");
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_TRAY_CALLBACK {
            match lparam.0 as u32 {
                WM_LBUTTONUP => {
                    if let Some(proxy) = PROXY.get() {
                        crate::log_err!(proxy.send(crate::ui::PhotonEvent::ShowWindow), "TRAY: waking the UI to show the window");
                    }
                }
                WM_RBUTTONUP => {
                    if let Ok(menu) = CreatePopupMenu() {
                        let show = wide("Show Photon");
                        let exit = wide("Exit");
                        crate::log_err!(AppendMenuW(menu, MF_STRING, MENU_SHOW, PCWSTR(show.as_ptr())), "TRAY: menu item Show append");
                        crate::log_err!(AppendMenuW(menu, MF_STRING, MENU_EXIT, PCWSTR(exit.as_ptr())), "TRAY: menu item Exit append");
                        let mut pt = POINT::default();
                        crate::log_err!(GetCursorPos(&mut pt), "TRAY: cursor position read");
                        // Required Win32 ritual: without SetForegroundWindow the popup never dismisses on outside-click.
                        // Its zero return sets no last-error code, so a refusal is said plainly.
                        if !SetForegroundWindow(hwnd).as_bool() {
                            crate::log("TRAY: SetForegroundWindow refused — the menu may not dismiss on an outside click");
                        }
                        let picked = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                            pt.x,
                            pt.y,
                            0,
                            hwnd,
                            None,
                        );
                        match picked.0 as usize {
                            MENU_SHOW => {
                                if let Some(proxy) = PROXY.get() {
                                    crate::log_err!(proxy.send(crate::ui::PhotonEvent::ShowWindow), "TRAY: waking the UI to show the window");
                                }
                            }
                            MENU_EXIT => {
                                // Killswitch-compliant, same as the SNI backend: the flock + control channel release with the process.
                                crate::log("TRAY: exit");
                                std::process::exit(0);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return LRESULT(0);
        }
        if Some(&msg) == TASKBAR_CREATED.get() {
            // Explorer restarted and took every tray icon with it — re-add ours.
            add_icon(hwnd);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    pub fn spawn(proxy: Arc<dyn WakeSender<crate::ui::PhotonEvent>>) {
        if PROXY.set(proxy).is_err() {
            return; // Already spawned — one icon per process.
        }
        std::thread::Builder::new()
            .name("tray".to_string())
            .spawn(|| unsafe {
                let Ok(hinstance) = GetModuleHandleW(None) else {
                    crate::log("TRAY: GetModuleHandleW failed — no tray this session");
                    return;
                };
                let class_name = wide("PhotonTrayClass");
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: hinstance.into(),
                    lpszClassName: PCWSTR(class_name.as_ptr()),
                    ..Default::default()
                };
                if RegisterClassW(&wc) == 0 {
                    crate::log("TRAY: window class registration failed — no tray this session");
                    return;
                }
                let tc = wide("TaskbarCreated");
                if TASKBAR_CREATED.set(RegisterWindowMessageW(PCWSTR(tc.as_ptr()))).is_err() {
                    crate::log("TRAY: the TaskbarCreated id was already registered — the first one stays");
                }
                // A plain hidden top-level window, NOT message-only (HWND_MESSAGE windows don't receive the TaskbarCreated broadcast).
                let Ok(hwnd) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    PCWSTR(class_name.as_ptr()),
                    PCWSTR(class_name.as_ptr()),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    hinstance,
                    None,
                ) else {
                    crate::log("TRAY: hidden window creation failed — no tray this session");
                    return;
                };
                if TRAY_HWND.set(hwnd.0 as usize).is_err() {
                    crate::log("TRAY: a tray window was already kept — the unread ring follows the first");
                }
                add_icon(hwnd);
                crate::log("TRAY: orb parked next to the clock (Shell_NotifyIcon; Windows may fold new icons into the ^ overflow until the user drags them out)");
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    // WHY: TranslateMessage's BOOL says whether a character message was generated — information, not a failure, and BOOL is must_use.
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .expect("tray thread spawn");
    }
}

/// Windows: Shell_NotifyIcon on a dedicated message-pump thread; left-click or menu "Show" surfaces the window, "Exit" quits, and the icon re-adds itself when Explorer restarts.
#[cfg(target_os = "windows")]
pub fn spawn(proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {
    windows_tray::spawn(proxy);
}

#[cfg(target_os = "macos")]
mod macos_tray {
    use std::sync::Arc;
    use std::sync::OnceLock;

    use fluor::host::WakeSender;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{define_class, msg_send, AllocAnyThread, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
    use objc2_foundation::{ns_string, NSObject, NSSize};

    /// The wake proxy — a process singleton like the Windows backend; the target object reads it from here so it stays `'static` without threading lifetimes thru objc.
    static PROXY: OnceLock<Arc<dyn WakeSender<crate::ui::PhotonEvent>>> = OnceLock::new();
    /// The status item + target, retained for the process (v1 parks them forever, same as the SNI handle).
    static PARKED: OnceLock<usize> = OnceLock::new();

    define_class!(
        // The action target for the status button + menu items — AppKit requires an objc object with selectors; this is the smallest one that can carry them.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "PhotonTrayTarget"]
        struct TrayTarget;

        impl TrayTarget {
            #[unsafe(method(showPhoton:))]
            fn show_photon(&self, _sender: Option<&AnyObject>) {
                if let Some(proxy) = PROXY.get() {
                    crate::log_err!(proxy.send(crate::ui::PhotonEvent::ShowWindow), "TRAY: waking the UI to show the window");
                }
            }

            #[unsafe(method(exitPhoton:))]
            fn exit_photon(&self, _sender: Option<&AnyObject>) {
                // Killswitch-compliant, same as the SNI + Shell_NotifyIcon backends.
                crate::log("TRAY: exit");
                std::process::exit(0);
            }
        }
    );

    /// The orb as an NSImage drawn `points` wide: an NSBitmapImageRep over a copy of `orb_rgba`'s straight rows, so no image file is encoded on the way.
    unsafe fn orb_image(asset: &[u8], unread: bool, points: f64) -> Option<Retained<NSImage>> {
        let (rgba, w, h) = super::orb_rgba(asset, unread)?;
        let rep: *mut AnyObject = msg_send![objc2::class!(NSBitmapImageRep), alloc];
        // Null planes: the rep allocates its own rows and ours are copied in below. Bitmap format 2 is NSBitmapFormatAlphaNonpremultiplied, which straight rows are.
        let rep: *mut AnyObject = msg_send![
            rep,
            initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
            pixelsWide: w as isize,
            pixelsHigh: h as isize,
            bitsPerSample: 8isize,
            samplesPerPixel: 4isize,
            hasAlpha: true,
            isPlanar: false,
            colorSpaceName: ns_string!("NSDeviceRGBColorSpace"),
            bitmapFormat: 2usize,
            bytesPerRow: (w * 4) as isize,
            bitsPerPixel: 32isize
        ];
        let rep = Retained::<AnyObject>::from_raw(rep)?;
        let rows: *mut u8 = msg_send![&*rep, bitmapData];
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), rows, rgba.len());
        let image = NSImage::initWithSize(NSImage::alloc(), NSSize { width: points, height: points });
        let _: () = msg_send![&*image, addRepresentation: &*rep];
        Some(image)
    }

    /// The status-bar orb and the Dock tile wear the unread ring, or drop it; the Dock goes back to the bundle's icon (nil restores it).
    pub fn refresh(unread: bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            crate::log("TRAY: the unread edge arrived off the main thread, so the orb keeps its ring state");
            return;
        };
        unsafe {
            let dock = if unread { orb_image(super::WINDOW_ORB, true, 256.) } else { None };
            NSApplication::sharedApplication(mtm).setApplicationIconImage(dock.as_deref());
            let Some(&raw) = PARKED.get() else {
                return;
            };
            let item = &*(raw as *const NSStatusItem);
            if let (Some(button), Some(image)) = (item.button(mtm), orb_image(super::TRAY_ORB, unread, 18.)) {
                button.setImage(Some(&image));
            }
        }
    }

    pub fn spawn(proxy: Arc<dyn WakeSender<crate::ui::PhotonEvent>>) {
        if PROXY.set(proxy).is_err() {
            return; // One icon per process.
        }
        // NSStatusItem is AppKit — main-thread-bound. spawn() is called from the UI thread, which winit REQUIRES to be the main thread on macOS, so this marker always resolves; the guard is belt-and-braces against a future off-thread caller.
        let Some(mtm) = MainThreadMarker::new() else {
            crate::log("TRAY: not on the main thread — no tray this session");
            return;
        };
        unsafe {
            let bar = NSStatusBar::systemStatusBar();
            // NSVariableStatusItemLength = -1.0; the square constant clips the orb on some bars.
            let item: Retained<NSStatusItem> = bar.statusItemWithLength(-1.0);
            let target = TrayTarget::alloc(mtm);
            let target: Retained<TrayTarget> = msg_send![target, init];
            if let Some(button) = item.button(mtm) {
                // Menu-bar icons render at ~18pt; drawing the orb that size keeps it from occupying a 64pt slab.
                if let Some(image) = orb_image(super::TRAY_ORB, super::UNREAD.load(super::Ordering::Relaxed), 18.) {
                    button.setImage(Some(&image));
                } else {
                    button.setTitle(ns_string!("\u{25CF}"));
                }
                let _: () = msg_send![&*button, setTarget: &*target];
                let _: () = msg_send![&*button, setAction: objc2::sel!(showPhoton:)];
            }
            let menu = NSMenu::new(mtm);
            let show = NSMenuItem::new(mtm);
            show.setTitle(ns_string!("Show Photon"));
            let _: () = msg_send![&*show, setTarget: &*target];
            let _: () = msg_send![&*show, setAction: objc2::sel!(showPhoton:)];
            menu.addItem(&show);
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            let exit = NSMenuItem::new(mtm);
            exit.setTitle(ns_string!("Exit"));
            let _: () = msg_send![&*exit, setTarget: &*target];
            let _: () = msg_send![&*exit, setAction: objc2::sel!(exitPhoton:)];
            menu.addItem(&exit);
            // With a menu attached, LEFT click also opens it — macOS convention (there is no separate left-activate once a menu is set, and fighting that needs a click-mask dance not worth it for v1). "Show Photon" is the top item, so surfacing is two clicks.
            item.setMenu(Some(&menu));
            // Park the retained objects for the process lifetime.
            if PARKED.set(Retained::into_raw(item) as usize).is_err() {
                crate::log("TRAY: a status item was already parked — this one stays leaked alongside it");
            }
            std::mem::forget(target);
        }
        crate::log("TRAY: orb parked in the menu bar (NSStatusItem)");
    }
}

/// macOS: NSStatusItem in the menu bar — the icon opens a Show/Exit menu (macOS convention once a menu is attached); main-thread-bound, called from the UI thread which IS the main thread under winit.
#[cfg(target_os = "macos")]
pub fn spawn(proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {
    macos_tray::spawn(proxy);
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn spawn(_proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {
    crate::log(
        "TRAY: no backend for this platform — residency still works via relaunch-to-surface",
    );
}

#[cfg(test)]
mod tests {
    /// The ringed orb keeps its corners empty, wears the unread magenta in the band outside the shrunken orb, and keeps the orb opaque at its centre.
    #[test]
    fn the_unread_orb_wears_a_magenta_band() {
        let (plain, w, h) = super::orb_rgba(super::TRAY_ORB, false).unwrap();
        let (ringed, rw, rh) = super::orb_rgba(super::TRAY_ORB, true).unwrap();
        assert_eq!((rw, rh, plain.len()), (w, h, ringed.len()));
        let at = |buf: &[u8], x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        };
        assert_eq!(at(&ringed, 0, 0)[3], 0);
        // The band runs from the orb's radius (three quarters of the icon's) out to the rim, the outer eighth of the width; a sixteenth in sits mid-band.
        let band = at(&ringed, w >> 4, h >> 1);
        let m = fluor::theme::to_sdr_visible(*crate::ui::theme::RING_UNREAD_COLOUR);
        // Within a few steps of the theme's magenta: fluor's 8-bit kernels divide by 256 where 255 is meant, the dark bias avatar_render's depth test pins too.
        assert_eq!(band[3], 255);
        for (got, want) in band.into_iter().zip([(m >> 16) as u8, (m >> 8) as u8, m as u8]) {
            assert!((got as i32 - want as i32).abs() <= 3, "band {band:?} against magenta {m:06x}");
        }
        assert_eq!(at(&ringed, w >> 1, h >> 1)[3], 255);
    }
}
