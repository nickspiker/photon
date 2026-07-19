//! Login-item management — the per-user, no-elevation autostart artifact on each desktop OS.
//! The artifact IS the setting: `enabled()` reads the OS state directly, `enable()`/`disable()` write/remove it, and nothing is stored in the vault — so the toggle survives reinstalls, can't desync from reality, and stays visible/revocable in the OS's own UI (macOS Login Items, Windows Task Manager Startup, GNOME Tweaks).
//! Every path here is user-owned (HKCU / ~/Library/LaunchAgents / ~/.config/autostart): no sudo, no UAC, no prompts — see the 2026-07-19 design discussion.
//! The registered command is `<current_exe> --background`, so a login launch comes up RESIDENT (hidden window, network up) rather than opening a window over the fresh session.

/// The command line the login item runs: the running binary, backgrounded. `current_exe` is honest about dev runs (a debug-target path) — registering from a dev build points autostart at that dev binary, which is what a developer iterating on this feature wants anyway. Installed builds resolve to `~/.local/bin/photon-messenger` (or the platform equivalent), which self-update swaps atomically in place, so the artifact never goes stale.
fn exe_path() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("current_exe: {e}"))
}

// ───────── Linux: XDG autostart entry ─────────
// A .desktop file in ~/.config/autostart — the freedesktop mechanism every session manager (GNOME, KDE, XFCE…) honours at graphical login. Session-scoped by design: a UI app wants the display + session env, which a systemd user unit only gets with extra ceremony.

#[cfg(target_os = "linux")]
fn artifact_path() -> Result<std::path::PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok_or("no XDG_CONFIG_HOME or HOME")?;
    Ok(base.join("autostart").join("photon-messenger.desktop"))
}

#[cfg(target_os = "linux")]
pub fn enabled() -> bool {
    artifact_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub fn enable() -> Result<(), String> {
    let path = artifact_path()?;
    let exe = exe_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Photon\nComment=Photon Messenger (background start)\nExec=\"{}\" --background\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    std::fs::write(&path, entry).map_err(|e| format!("write {}: {e}", path.display()))
}

#[cfg(target_os = "linux")]
pub fn disable() -> Result<(), String> {
    let path = artifact_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {}: {e}", path.display())),
    }
}

// ───────── macOS: LaunchAgent plist ─────────
// ~/Library/LaunchAgents/com.photon.messenger.plist with RunAtLoad. Takes effect at next login without launchctl (launchd scans the directory); the SMAppService route needs a signed .app bundle we don't ship yet.

#[cfg(target_os = "macos")]
fn artifact_path() -> Result<std::path::PathBuf, String> {
    std::env::var_os("HOME")
        .map(|h| {
            std::path::PathBuf::from(h)
                .join("Library/LaunchAgents")
                .join("com.photon.messenger.plist")
        })
        .ok_or_else(|| "no HOME".to_string())
}

#[cfg(target_os = "macos")]
pub fn enabled() -> bool {
    artifact_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn enable() -> Result<(), String> {
    let path = artifact_path()?;
    let exe = exe_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    }
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Label</key>\n\t<string>com.photon.messenger</string>\n\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{}</string>\n\t\t<string>--background</string>\n\t</array>\n\t<key>RunAtLoad</key>\n\t<true/>\n</dict>\n</plist>\n",
        exe.display()
    );
    std::fs::write(&path, plist).map_err(|e| format!("write {}: {e}", path.display()))
}

#[cfg(target_os = "macos")]
pub fn disable() -> Result<(), String> {
    let path = artifact_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {}: {e}", path.display())),
    }
}

// ───────── Windows: HKCU Run value ─────────
// The per-user registry Run key, written via reg.exe (in every Windows install; keeps us dependency-free). Shows in Task Manager → Startup where the user can also disable it.

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const RUN_VALUE: &str = "PhotonMessenger";

#[cfg(target_os = "windows")]
pub fn enabled() -> bool {
    std::process::Command::new("reg")
        .args(["query", RUN_KEY, "/v", RUN_VALUE])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
pub fn enable() -> Result<(), String> {
    let exe = exe_path()?;
    let cmd = format!("\"{}\" --background", exe.display());
    let out = std::process::Command::new("reg")
        .args(["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &cmd, "/f"])
        .output()
        .map_err(|e| format!("reg add: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("reg add failed: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

#[cfg(target_os = "windows")]
pub fn disable() -> Result<(), String> {
    let out = std::process::Command::new("reg")
        .args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])
        .output()
        .map_err(|e| format!("reg delete: {e}"))?;
    // "unable to find" = already absent = success for our purposes.
    if out.status.success() || String::from_utf8_lossy(&out.stderr).contains("unable to find") {
        Ok(())
    } else {
        Err(format!("reg delete failed: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

// ───────── Anything else desktop-ish (Redox…) — no autostart mechanism wired ─────────

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn enabled() -> bool {
    false
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn enable() -> Result<(), String> {
    Err("autostart not supported on this platform".to_string())
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn disable() -> Result<(), String> {
    Ok(())
}
