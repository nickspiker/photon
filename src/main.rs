// Hide console window on Windows
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use photon_messenger::crypto::self_verify;
use photon_messenger::ui::photon_app::PhotonApp;

fn main() {
    // Initialize logging (redirects stdout/stderr to file on Windows GUI apps)
    photon_messenger::init_logging();
    // FIRST log line: which build is this? Every submitted log now self-identifies its version + commit.
    photon_messenger::log_version();

    // Sweep any swap leftovers from a prior update — the Windows .old shuffle, a torn .update-staged, and the "(deleted)"-suffixed litter the pre-fix updater could leave — so a mis-installed tree self-heals on launch (this was defined but never called before).
    photon_messenger::network::updates::sweep_old_binary();

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

        // SIDECAR FIRST, lock-free: a panic raised inside the log sink self-deadlocks the calls below (non-reentrant mutexes) and the evidence dies with the process.
        photon_messenger::write_crash_sidecar(&format!("PANIC at {}: {}", location, msg));
        photon_messenger::logf!("PANIC at {}: {}", location, msg);
        // A panic is THE flush edge: the process is about to die and in-process RAM (the soft-mode batch) dies with it.
        photon_messenger::flush_log_buffer();

        // Also print backtrace if available
        let backtrace = std::backtrace::Backtrace::capture();
        if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
            photon_messenger::write_crash_sidecar(&format!("Backtrace:\n{}", backtrace));
            photon_messenger::logf!("Backtrace:\n{}", backtrace);
        }
    }));

    // Last run's crash, if any, folded into this run's log so it rides the next submission.
    photon_messenger::report_prior_crash();
    // Native faults (access violation / SIGSEGV / stack overflow) bypass the panic hook on every OS — this writes the same sidecar so they ride the next submission too.
    photon_messenger::platform::crash_native::install();

    // Check for verify argument (used by install script to validate binary)
    let verify_only = std::env::args().any(|arg| arg == "verify");

    // Test panic hook with test-panic argument
    if std::env::args().any(|arg| arg == "test-panic") {
        photon_messenger::log("Testing panic hook...");
        panic!("TEST PANIC - this should appear in the log");
    }

    // BRIDGE needs no host opt-in (Nick's ruling 2026-08-21): the fold-verified, non-locked sibling gate IS the authorization — the old --enable/--disable-remote-terminal flag pair is gone (its census-wiped marker had silently darkened every host: the Europe incident's second half). A locked-out device is the one thing the bridge refuses.

    // Resilient-launch handoff (docs/resilient-launch.md): the launch shim already ran `photon verify` on THIS exact file microseconds ago, so it relaunched us with PHOTON_LAUNCH_VERIFIED set — skip the redundant startup self-check so EXACTLY ONE validation runs per launch. CONSUME it immediately: photon inherits its environment into child processes (the self-update re-exec among them, which runs a freshly-downloaded binary), and this skip must never leak past the single shim→photon hop, or an update would install and run unverified.
    let launcher_prevalidated = std::env::var_os("PHOTON_LAUNCH_VERIFIED").is_some();
    if launcher_prevalidated {
        std::env::remove_var("PHOTON_LAUNCH_VERIFIED");
    }

    // Verify binary signature matches fractaldecoder (Ed25519 cryptographic signature) — unless the shim already did, and this isn't an explicit `verify` request (which must ALWAYS check, it IS the shim's check).
    let signature_hex = if launcher_prevalidated && !verify_only {
        "(verified by launch shim)".to_string()
    } else {
        match self_verify::verify_binary_hash() {
            Ok(sig) => sig,
            Err(e) => {
                photon_messenger::logf!("BINARY INTEGRITY CHECK FAILED: {}", e);
                photon_messenger::log("");
                photon_messenger::log("This usually means:");
                photon_messenger::log("  - Download was corrupted or incomplete");
                photon_messenger::log("  - Storage failure (bad sectors, bit flips)");
                photon_messenger::log("  - Binary was modified or tampered with");
                photon_messenger::log("");
                photon_messenger::log("Try reinstalling from: https://holdmyoscilloscope.com/photon");
                std::process::exit(1);
            }
        }
    };

    // If verify argument, exit successfully (used by install script)
    if verify_only {
        println!("OK");
        std::process::exit(0);
    }

    // Headless lifeline mode (docs/headless-lifeline.md): no fluor, no winit, no display — the bridge host that survives X death. Runs as a WATCHER while a real instance holds the lock, becomes the instance the moment the lock frees, and yields it back when a full-UI launch asks.
    let lifeline_mode = std::env::args().any(|arg| arg == "--lifeline");

    // Single-instance guard: a second instance on the SAME data dir would race the vault and corrupt the log.
    // Held for the whole process (OS frees it on exit). A second instance with its own PHOTON_DATA_DIR (+ PHOTON_FINGERPRINT for a distinct identity) hashes to a different lock port and is allowed — that's the supported way to run two parties on one machine.
    // Losing the lock is no longer an error by default: the resident-mode handoff — clicking the icon while a (possibly hidden) instance runs — asks that instance to surface itself and exits quietly; if the holder is a LIFELINE it yields the lock instead and this launch takes over. The old already-running error remains the fallback when nobody answers the control channel.
    let dir = photon_messenger::storage::photon_config_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let install_listener = |lock: &photon_messenger::storage::InstanceLock| {
        // We ARE the instance: park the control listener for the app to serve once its event proxy exists. Unix gets a dedicated socket (safe to create only now, under the flock); Windows reuses the lock's own TcpListener.
        #[cfg(unix)]
        photon_messenger::platform::control::install_unix_listener(&dir);
        #[cfg(not(unix))]
        if let Some(l) = lock.control_listener() {
            photon_messenger::platform::control::install_tcp_listener(l);
        }
    };
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    if lifeline_mode {
        // WATCHER: while a real instance runs, do nothing but wait for the lock to free (X death takes the instance and its flock with it). Liveness polling of a lock file is supervisor work, not UI work — the edges-not-timers rule governs the product, not the janitor.
        let lock = loop {
            match photon_messenger::storage::acquire_single_instance(&dir) {
                Some(lock) => break lock,
                None => std::thread::sleep(std::time::Duration::from_secs(5)),
            }
        };
        install_listener(&lock);
        photon_messenger::log("LIFELINE: lock acquired — starting the headless pump");
        photon_messenger::ui::photon_app::lifeline::run_lifeline(PhotonApp::new());
        // Yield: return releases the flock; the full-UI launch retrying acquire takes over. Exit 0 so a supervising unit restarts us back into watcher mode.
        photon_messenger::flush_log_buffer();
        std::process::exit(0);
    }
    #[cfg(not(all(unix, not(target_os = "android"), not(target_os = "redox"))))]
    if lifeline_mode {
        eprintln!("photon: --lifeline is desktop-unix only");
        std::process::exit(1);
    }
    let _instance_lock = {
        match photon_messenger::storage::acquire_single_instance(&dir) {
            Some(lock) => {
                install_listener(&lock);
                lock
            }
            None => {
                // The holder may be a lifeline (docs/headless-lifeline.md): ask it to yield, then retry the lock briefly — a lifeline exits and frees it; a full-UI resident keeps it (and surfaces), so the retry fails and we fall thru to the show path.
                let mut took_over = None;
                if photon_messenger::platform::control::request_yield(&dir) {
                    for _ in 0..50 {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        if let Some(lock) = photon_messenger::storage::acquire_single_instance(&dir) {
                            took_over = Some(lock);
                            break;
                        }
                    }
                }
                match took_over {
                    Some(lock) => {
                        photon_messenger::log("LIFELINE: took the lock over from a yielding lifeline instance");
                        install_listener(&lock);
                        lock
                    }
                    None => {
                        if photon_messenger::platform::control::request_show(&dir) {
                            println!(
                                "photon: already running — asked the resident instance to show itself."
                            );
                            std::process::exit(0);
                        }
                        eprintln!(
                            "photon: another instance is already running for this data dir:\n  {}\nFor a second instance (two-party testing) set a separate PHOTON_DATA_DIR (and PHOTON_FINGERPRINT for a distinct identity).",
                            dir.display()
                        );
                        std::process::exit(1);
                    }
                }
            }
        }
    };

    photon_messenger::logf!("SIGNATURE CHECK PASSED");
    photon_messenger::logf!("Ed25519 signature: {}", signature_hex);
    photon_messenger::log("");

    // Startup message
    photon_messenger::log("Photon Messenger - Distilled to what messaging actually requires, for true data sovereignty");
    photon_messenger::log("by Nick Spiker <fractaldecoder@proton.me>");
    photon_messenger::log("");
    photon_messenger::log(
        "I built this to give you the best damn secure messaging experience possible.",
    );
    photon_messenger::log("Your data belongs to you—no servers, no tracking, no compromises.");
    photon_messenger::log("");
    photon_messenger::log("Found a bug? Have feedback? Email me: fractaldecoder@proton.me");
    photon_messenger::log("(Photon messenger coming soon—for now there's only ~3 of us!)");
    photon_messenger::log("");

    // Route the `log` crate (fluor and friends) into the VSF sink — no stdout fork; read it live with `photonlog -f`.
    photon_messenger::install_log_bridge();

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

    // Hand off to fluor's host. PhotonApp::new() is parameterless: the host hands us the event-loop proxy via FluorApp::set_event_proxy and the initial viewport via FluorApp::init, so there's nothing to thread thru up-front.
    fluor::host::app::run_app(PhotonApp::new()).expect("event loop failed");
}
