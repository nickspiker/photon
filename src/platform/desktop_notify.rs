//! Desktop system notifications — the "ding while you're not looking" analog of Android's `notify_new_message`.
//! Fired POST-DECRYPT from the UI receive path, so the banner carries the sender's display name and the message text BY DESIGN — hiding content on the lock screen is the OS notification daemon's job, not ours. The pre-decrypt RX worker carries nothing because it no longer notifies (probes and sibling fleet-sync frames used to over-ding from there).
//! Zero-dependency by shelling to each platform's stock notifier (`notify-send` / `osascript` / PowerShell's WinRT toast) — the processes are fire-and-forget and their absence (minimal server installs) degrades to silence, never an error.
//! Gated on the window being HIDDEN or UNFOCUSED — a notification about the conversation you're looking at is noise. The two flags live here as atomics because historically the decision point (the status RX worker) was not the UI thread that owns the truth, and any thread may still call in.

use std::sync::atomic::{AtomicBool, Ordering};

/// Window visibility as the UI thread last reported it (`false` after a resident-close hide or a `--background` start). Fail-visible default: until the app reports otherwise we assume someone's looking and stay quiet.
static WINDOW_VISIBLE: AtomicBool = AtomicBool::new(true);
/// OS keyboard-focus state, same reporting discipline.
static WINDOW_FOCUSED: AtomicBool = AtomicBool::new(true);

pub fn set_window_visible(visible: bool) {
    WINDOW_VISIBLE.store(visible, Ordering::Relaxed);
}

pub fn set_window_focused(focused: bool) {
    WINDOW_FOCUSED.store(focused, Ordering::Relaxed);
}

/// True when the window is both visible AND focused — someone is plausibly looking at the app. The unread-count gate reads this alongside "is this conversation the active view" so a message landing behind a hidden or unfocused window still marks unread.
pub fn window_attended() -> bool {
    WINDOW_VISIBLE.load(Ordering::Relaxed) && WINDOW_FOCUSED.load(Ordering::Relaxed)
}

/// Fleet-attention mirror (2026-08-18): true when THIS device holds the fleet's newest human input — or nobody does (bootstrap default, matching the fail-visible discipline above). The UI thread owns `fleet_attention` and mirrors every mutation here so the notify gate can read it from any thread. A focused-but-abandoned window (human demonstrably at another device) must still banner.
static ATTENTION_OURS: AtomicBool = AtomicBool::new(true);

pub fn set_attention_ours(ours: bool) {
    ATTENTION_OURS.store(ours, Ordering::Relaxed);
}

/// The last message identity we notified for, mirroring the Android dedupe: a dozing peer's retransmits redeliver the SAME logical message many times, and each would otherwise re-ding. Keyed on the message's chain-derived hash pointer, unique per logical message.
static LAST_NOTIFIED: std::sync::Mutex<[u8; 32]> = std::sync::Mutex::new([0u8; 32]);

/// The UI's wake handle, so a CLICKED notification can surface the window (the same ShowWindow the tray sends). Set once when the event loop hands the app its proxy.
static WAKE: std::sync::OnceLock<std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>> = std::sync::OnceLock::new();

/// The conversation the last clicked notification named, taken by the app's tick.
static PENDING_OPEN: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);

pub fn set_wake(proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {
    let _ = WAKE.set(proxy);
}

/// One-shot take of the conversation a notification click asked to open.
pub fn take_open_conversation() -> Option<[u8; 32]> {
    PENDING_OPEN.lock().ok().and_then(|mut g| g.take())
}

/// A notification was CLICKED: remember what it named and bring the window up; the tick opens it.
#[allow(dead_code)] // reached only from the platforms whose notifier reports a click (Linux)
fn clicked(conversation: Option<[u8; 32]>) {
    if let (Some(id), Ok(mut g)) = (conversation, PENDING_OPEN.lock()) {
        *g = Some(id);
    }
    crate::log("NOTIFY: desktop alert clicked — surfacing the window");
    if let Some(w) = WAKE.get() {
        let _ = w.send(crate::ui::PhotonEvent::ShowWindow);
    }
}

/// Fire the platform notification for a decrypted message — title = the sender's display name, body = the message text — if anyone could actually be missing it (window hidden or unfocused) and this message hasn't already dinged. Callable from any thread.
/// `conversation`: what a click opens, where the platform's notifier reports clicks (`None` = the window just comes up).
pub fn notify_new_message(msg_hp: &[u8; 32], sender: &str, text: &str, conversation: Option<[u8; 32]>) {
    // Every outcome logs (Nick's MacBook 2026-10-20: nothing in the log could say whether an alert was ever posted).
    if window_attended() && ATTENTION_OURS.load(Ordering::Relaxed) {
        crate::log("NOTIFY: desktop alert suppressed — the window is attended");
        return;
    }
    {
        let mut last = LAST_NOTIFIED.lock().unwrap();
        if *last == *msg_hp {
            crate::log("NOTIFY: desktop alert suppressed — this photon already alerted");
            return;
        }
        *last = *msg_hp;
    }
    crate::log("NOTIFY: desktop alert posted");
    post(sender, text, conversation);
}

#[cfg(target_os = "linux")]
fn post(title: &str, body: &str, conversation: Option<[u8; 32]>) {
    // CLICKABLE: `--action` + `--wait` keep notify-send running until the notification closes and print the chosen action, so a click opens its conversation. It runs on its own thread; a libnotify too old for actions exits at once with nothing printed, and the plain fire-and-forget form posts instead.
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        let waited = std::process::Command::new("notify-send")
            .args(["--app-name=Photon", "--action=default=Open", "--wait", &title, &body])
            .stderr(std::process::Stdio::null())
            .output();
        match waited {
            Ok(out) if out.status.success() => {
                if String::from_utf8_lossy(&out.stdout).trim() == "default" {
                    clicked(conversation);
                }
            }
            Ok(_) => {
                let _ = std::process::Command::new("notify-send")
                    .args(["--app-name=Photon", &title, &body])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            Err(e) => crate::logf!("NOTIFY: the system notifier failed to launch: {}", e),
        }
    });
}

#[cfg(target_os = "macos")]
fn post(title: &str, body: &str, _conversation: Option<[u8; 32]>) {
    // `display notification` needs no bundle/signing/notarization; attribution shows as Script Editor until we ship a proper .app with UNUserNotificationCenter.
    // Title/body are now real sender/message text interpolated into an AppleScript string literal, so backslashes and quotes MUST be escaped or a message containing them breaks (or injects into) the script.
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let (title, body) = (esc(title), esc(body));
    let script = format!("display notification \"{body}\" with title \"{title}\"");
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| crate::logf!("NOTIFY: the system notifier failed to launch: {}", e));
}

#[cfg(target_os = "windows")]
fn post(title: &str, body: &str, _conversation: Option<[u8; 32]>) {
    // WinRT toast via PowerShell — attribution rides PowerShell's AppUserModelID until we register our own (needs a Start-menu shortcut with an AUMID; packaging-time work). -WindowStyle Hidden keeps the transient console from flashing.
    // Title/body are now real sender/message text landing inside PowerShell single-quoted literals: double the single quotes so message content can't terminate the literal.
    let esc = |s: &str| s.replace('\'', "''");
    let (title, body) = (esc(title), esc(body));
    let ps = format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; \
         $x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
         $t = $x.GetElementsByTagName('text'); \
         $t.Item(0).AppendChild($x.CreateTextNode('{title}')) | Out-Null; \
         $t.Item(1).AppendChild($x.CreateTextNode('{body}')) | Out-Null; \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Microsoft.Windows.PowerShell').Show([Windows.UI.Notifications.ToastNotification]::new($x))"
    );
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| crate::logf!("NOTIFY: the system notifier failed to launch: {}", e));
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn post(_title: &str, _body: &str, _conversation: Option<[u8; 32]>) {}
