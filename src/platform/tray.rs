//! System-tray presence for desktop resident mode — the thing next to the clock. Left-click (or menu "Show") surfaces the hidden window thru the same `PhotonEvent::ShowWindow` path the second-launch handoff uses; "Exit" is THE deliberate quit affordance residency was missing (close hides, tray exits). The icon is the round orb — same circular-mask discipline as `fluor::host::icon::Icon::to_rgba_circular`, sourced from the shipped round asset so tray and taskbar can't disagree.
//!
//! Linux: StatusNotifierItem via ksni (pure Rust zbus — no GTK, cross-compiles clean). GNOME needs the AppIndicator extension to SHOW SNI items (KDE/XFCE show them natively); without it the icon simply doesn't appear and nothing else breaks — resident behaviour still works via the second-launch handoff.
//! Windows (Shell_NotifyIcon) and macOS (NSStatusItem, main-thread-bound) are the next backends; until then `spawn` logs and returns, and residency works without a tray there too.

#[cfg(target_os = "linux")]
mod linux {
    use fluor::host::WakeSender;
    use std::sync::Arc;

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
            // The shipped round RGBA asset (transparent corners, AA rim) → SNI's network-byte-order ARGB32.
            let Ok(img) = image::load_from_memory(include_bytes!("../../assets/icon-64.png")) else {
                return Vec::new();
            };
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width() as i32, rgba.height() as i32);
            let mut argb = Vec::with_capacity((w * h * 4) as usize);
            for px in rgba.pixels() {
                let [r, g, b, a] = px.0;
                argb.extend_from_slice(&[a, r, g, b]);
            }
            vec![ksni::Icon { width: w, height: h, data: argb }]
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.proxy.send(crate::ui::PhotonEvent::ShowWindow);
        }
        fn menu(&self) -> Vec<ksni::menu::MenuItem<Self>> {
            use ksni::menu::*;
            vec![
                StandardItem {
                    label: "Show Photon".into(),
                    activate: Box::new(|t: &mut Self| {
                        let _ = t.proxy.send(crate::ui::PhotonEvent::ShowWindow);
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
                // The handle is the update/shutdown capability; the icon lives for the process in v1 (despawn-on-toggle-off comes with a handle plumb-thru), so park it forever.
                std::mem::forget(handle);
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
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DispatchMessageW, GetCursorPos, GetMessageW, RegisterClassW, RegisterWindowMessageW,
        SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON, HMENU, ICONINFO, MF_STRING,
        MSG, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE,
        WINDOW_STYLE, WM_APP, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
    };

    /// The tray thread's wake proxy — a process singleton because the WndProc has no instance pointer worth threading for one icon.
    static PROXY: OnceLock<Arc<dyn WakeSender<crate::ui::PhotonEvent>>> = OnceLock::new();
    /// Explorer's "TaskbarCreated" broadcast id — a shell restart destroys every tray icon, and re-adding on this message is how an icon survives it.
    static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

    const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
    const MENU_SHOW: usize = 1;
    const MENU_EXIT: usize = 2;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Build an HICON from the shipped round orb RGBA — CreateIconIndirect over a 32bpp BGRA colour bitmap plus an unused-but-required mask.
    unsafe fn orb_icon() -> Option<HICON> {
        let img = image::load_from_memory(include_bytes!("../../assets/icon-64.png")).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width() as i32, rgba.height() as i32);
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for px in rgba.pixels() {
            let [r, g, b, a] = px.0;
            bgra.extend_from_slice(&[b, g, r, a]);
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
        let _ = DeleteObject(colour);
        let _ = DeleteObject(mask);
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
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_TRAY_CALLBACK {
            match lparam.0 as u32 {
                WM_LBUTTONUP => {
                    if let Some(proxy) = PROXY.get() {
                        let _ = proxy.send(crate::ui::PhotonEvent::ShowWindow);
                    }
                }
                WM_RBUTTONUP => {
                    if let Ok(menu) = CreatePopupMenu() {
                        let show = wide("Show Photon");
                        let exit = wide("Exit");
                        let _ = AppendMenuW(menu, MF_STRING, MENU_SHOW, PCWSTR(show.as_ptr()));
                        let _ = AppendMenuW(menu, MF_STRING, MENU_EXIT, PCWSTR(exit.as_ptr()));
                        let mut pt = POINT::default();
                        let _ = GetCursorPos(&mut pt);
                        // Required Win32 ritual: without SetForegroundWindow the popup never dismisses on outside-click.
                        let _ = SetForegroundWindow(hwnd);
                        let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
                        match picked.0 as usize {
                            MENU_SHOW => {
                                if let Some(proxy) = PROXY.get() {
                                    let _ = proxy.send(crate::ui::PhotonEvent::ShowWindow);
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
                let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(PCWSTR(tc.as_ptr())));
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
                add_icon(hwnd);
                crate::log("TRAY: orb parked next to the clock (Shell_NotifyIcon; Windows may fold new icons into the ^ overflow until the user drags them out)");
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
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
    use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
    use objc2_foundation::{ns_string, NSData, NSObject, NSSize};

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
                    let _ = proxy.send(crate::ui::PhotonEvent::ShowWindow);
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
                let bytes = include_bytes!("../../assets/icon-64.png");
                let data = NSData::with_bytes(bytes);
                if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
                    // Menu-bar icons render at ~18pt; setting the drawn size keeps the orb from occupying a 64pt slab.
                    image.setSize(NSSize { width: 18.0, height: 18.0 });
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
            let _ = PARKED.set(Retained::into_raw(item) as usize);
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
    crate::log("TRAY: no backend for this platform — residency still works via relaunch-to-surface");
}
