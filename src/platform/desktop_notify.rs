//! Desktop system notifications — the "ding while you're not looking" analog of Android's `notify_new_message`, with the same privacy stance: the banner is a generic "New message"; no sender name, handle, pubkey, or plaintext ever reaches the OS notification daemon (which logs, previews on lock screens, and syncs to who-knows-where).
//! Zero-dependency by shelling to each platform's stock notifier (`notify-send` / `osascript` / PowerShell's WinRT toast) — the processes are fire-and-forget and their absence (minimal server installs) degrades to silence, never an error.
//! Gated on the window being HIDDEN or UNFOCUSED — a notification about the conversation you're looking at is noise. The two flags live here as atomics because the decision point (the status RX worker) is not the UI thread that owns the truth.

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

/// The last message identity we notified for, mirroring the Android dedupe: a dozing peer's retransmits redeliver the SAME logical message many times, and each would otherwise re-ding. Keyed on the message's chain position (`prev_msg_hp`), unique per logical message.
static LAST_NOTIFIED: std::sync::Mutex<[u8; 32]> = std::sync::Mutex::new([0u8; 32]);

/// Fire the platform "New message" notification, if anyone could actually be missing the message (window hidden or unfocused) and this message hasn't already dinged. Callable from any thread.
pub fn notify_new_message(msg_hp: &[u8; 32]) {
    if WINDOW_VISIBLE.load(Ordering::Relaxed) && WINDOW_FOCUSED.load(Ordering::Relaxed) {
        return;
    }
    {
        let mut last = LAST_NOTIFIED.lock().unwrap();
        if *last == *msg_hp {
            return;
        }
        *last = *msg_hp;
    }
    post("Photon", "New message");
}

#[cfg(target_os = "linux")]
fn post(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=Photon", title, body])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(target_os = "macos")]
fn post(title: &str, body: &str) {
    // `display notification` needs no bundle/signing/notarization; attribution shows as Script Editor until we ship a proper .app with UNUserNotificationCenter.
    let script = format!("display notification \"{body}\" with title \"{title}\"");
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(target_os = "windows")]
fn post(title: &str, body: &str) {
    // WinRT toast via PowerShell — attribution rides PowerShell's AppUserModelID until we register our own (needs a Start-menu shortcut with an AUMID; packaging-time work). -WindowStyle Hidden keeps the transient console from flashing.
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
        .spawn();
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn post(_title: &str, _body: &str) {}
