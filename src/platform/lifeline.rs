// Lifeline enrolment (docs/headless-lifeline.md) — the "bulletproof bridge" checkbox's platform half, mirroring autostart.rs doctrine: the artifact IS the setting, exe_path is dev-honest, absence reads as disabled.
// Linux: a systemd user unit (the user manager provably survives session death — the 2026-09-04 incident ran three days on it). macOS: a LaunchAgent with KeepAlive. Windows/Android/Redox: no --lifeline (the flag itself is cfg'd out), so this module is too.

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exe_path() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("current_exe: {e}"))
}

#[cfg(target_os = "linux")]
fn unit_path() -> Result<std::path::PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok_or("no XDG_CONFIG_HOME or HOME")?;
    Ok(base.join("systemd/user/photon-lifeline.service"))
}

#[cfg(target_os = "linux")]
pub fn enabled() -> bool {
    unit_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub fn enable() -> Result<(), String> {
    let path = unit_path()?;
    let exe = exe_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    }
    let unit = format!(
        "[Unit]\nDescription=Photon headless lifeline (bridge survives X death — docs/headless-lifeline.md)\n\n[Service]\nExecStart=\"{}\" --lifeline\nRestart=always\nRestartSec=10\n\n[Install]\nWantedBy=default.target\n",
        exe.display()
    );
    std::fs::write(&path, unit).map_err(|e| format!("write {}: {e}", path.display()))?;
    // Best-effort immediate arm — the artifact alone covers the next login; these make it live NOW.
    let _ = std::process::Command::new("systemctl").args(["--user", "daemon-reload"]).status();
    let ok = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", "photon-lifeline.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        crate::log("LIFELINE: unit written but systemctl enable --now failed — it arms at next login");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn disable() -> Result<(), String> {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", "photon-lifeline.service"])
        .status();
    let path = unit_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => {
            let _ = std::process::Command::new("systemctl").args(["--user", "daemon-reload"]).status();
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {}: {e}", path.display())),
    }
}

#[cfg(target_os = "macos")]
fn agent_path() -> Result<std::path::PathBuf, String> {
    std::env::var_os("HOME")
        .map(|h| {
            std::path::PathBuf::from(h)
                .join("Library/LaunchAgents")
                .join("com.photon.lifeline.plist")
        })
        .ok_or_else(|| "no HOME".to_string())
}

#[cfg(target_os = "macos")]
pub fn enabled() -> bool {
    agent_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn enable() -> Result<(), String> {
    let path = agent_path()?;
    let exe = exe_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    }
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Label</key><string>com.photon.lifeline</string>\n\t<key>ProgramArguments</key><array><string>{}</string><string>--lifeline</string></array>\n\t<key>RunAtLoad</key><true/>\n\t<key>KeepAlive</key><true/>\n</dict>\n</plist>\n",
        exe.display()
    );
    std::fs::write(&path, plist).map_err(|e| format!("write {}: {e}", path.display()))?;
    // Best-effort immediate arm; launchd scans LaunchAgents at login regardless.
    let _ = std::process::Command::new("launchctl").args(["load", "-w"]).arg(&path).status();
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn disable() -> Result<(), String> {
    let path = agent_path()?;
    let _ = std::process::Command::new("launchctl").args(["unload", "-w"]).arg(&path).status();
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {}: {e}", path.display())),
    }
}
