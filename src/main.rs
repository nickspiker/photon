// Hide console window on Windows
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use photon_messenger::crypto::self_verify;
use photon_messenger::ui::photon_app::PhotonApp;

fn main() {
    // Initialize logging (redirects stdout/stderr to file on Windows GUI apps)
    photon_messenger::init_logging();

    // Set up panic hook to log panics to file (critical for debugging Windows GUI crashes)
    std::panic::set_hook(Box::new(|panic_info| {
        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };

        let location = if let Some(loc) = panic_info.location() {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        } else {
            "unknown location".to_string()
        };

        photon_messenger::log(&format!("PANIC at {}: {}", location, msg));

        // Also print backtrace if available
        let backtrace = std::backtrace::Backtrace::capture();
        if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
            photon_messenger::log(&format!("Backtrace:\n{}", backtrace));
        }
    }));

    // Check for verify argument (used by install script to validate binary)
    let verify_only = std::env::args().any(|arg| arg == "verify");

    // Test panic hook with test-panic argument
    if std::env::args().any(|arg| arg == "test-panic") {
        photon_messenger::log("Testing panic hook...");
        panic!("TEST PANIC - this should appear in the log");
    }

    // Verify binary signature matches fractaldecoder (Ed25519 cryptographic signature)
    let signature_hex = match self_verify::verify_binary_hash() {
        Ok(sig) => sig,
        Err(e) => {
            photon_messenger::log(&format!("BINARY INTEGRITY CHECK FAILED: {}", e));
            photon_messenger::log("");
            photon_messenger::log("This usually means:");
            photon_messenger::log("  - Download was corrupted or incomplete");
            photon_messenger::log("  - Storage failure (bad sectors, bit flips)");
            photon_messenger::log("  - Binary was modified or tampered with");
            photon_messenger::log("");
            photon_messenger::log("Try reinstalling from: https://holdmyoscilloscope.com/photon");
            std::process::exit(1);
        }
    };

    // If verify argument, exit successfully (used by install script)
    if verify_only {
        println!("OK");
        std::process::exit(0);
    }

    photon_messenger::log(&format!("SIGNATURE CHECK PASSED"));
    photon_messenger::log(&format!("Ed25519 signature: {}", signature_hex));
    photon_messenger::log("");

    // Startup message
    photon_messenger::log("Photon Messenger - Built from first principles for true data sovereignty");
    photon_messenger::log("by Nick Spiker <fractaldecoder@proton.me>");
    photon_messenger::log("");
    photon_messenger::log("I built this to give you the best damn secure messaging experience possible.");
    photon_messenger::log("Your data belongs to you—no servers, no tracking, no compromises.");
    photon_messenger::log("");
    photon_messenger::log("Found a bug? Have feedback? Email me: fractaldecoder@proton.me");
    photon_messenger::log("(Photon messenger coming soon—for now there's only ~3 of us!)");
    photon_messenger::log("");

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Set cursor size for Linux/X11 to match system cursor settings. Winit doesn't read the DE cursor size, so we set it manually before fluor's host opens its window.
    #[cfg(target_os = "linux")]
    {
        if std::env::var("XCURSOR_SIZE").is_err() {
            // Try to read from GNOME/KDE settings, fallback to 24 (X11 default)
            let cursor_size = std::process::Command::new("gsettings")
                .args(&["get", "org.gnome.desktop.interface", "cursor-size"])
                .output()
                .ok()
                .and_then(|output| {
                    String::from_utf8(output.stdout)
                        .ok()
                        .and_then(|s| s.trim().parse::<u32>().ok())
                })
                .unwrap_or(24);

            std::env::set_var("XCURSOR_SIZE", cursor_size.to_string());
        }
    }

    // Hand off to fluor's host. PhotonApp::new() is parameterless: the host hands us the event-loop proxy via FluorApp::set_event_proxy and the initial viewport via FluorApp::init, so there's nothing to thread through up-front.
    fluor::host::app::run_app(PhotonApp::new()).expect("event loop failed");
}
