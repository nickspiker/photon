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

#[cfg(not(target_os = "linux"))]
pub fn spawn(_proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {
    crate::log("TRAY: no backend for this platform yet (Windows Shell_NotifyIcon / macOS NSStatusItem pending) — residency still works via relaunch-to-surface");
}
