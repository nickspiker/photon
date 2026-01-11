// Hide console window on Windows
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use photon_messenger::crypto::self_verify;
use photon_messenger::debug_println;
use photon_messenger::ui::{PhotonApp, PhotonEvent};

use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};

struct App {
    window: Option<Window>,
    photon_app: Option<PhotonApp>,
    screen_width: u32,
    screen_height: u32,
    maximized_size: Option<(u32, u32)>, // Maximized dimensions (learned on first maximize)
    blinkey_blink_rate_ms: u64,         // System blinkey blink rate in milliseconds
    event_proxy: EventLoopProxy<PhotonEvent>, // For cross-thread wake
}

impl ApplicationHandler<PhotonEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            // Get primary monitor size
            let monitor = event_loop
                .primary_monitor()
                .or_else(|| event_loop.available_monitors().next())
                .expect("No monitor found");

            let screen_size = monitor.size();
            let screen_width = screen_size.width;
            let screen_height = screen_size.height;

            // Store screen dimensions
            self.screen_width = screen_width;
            self.screen_height = screen_height;

            // Query monitor refresh rate and calculate target frame duration
            // Floor the value so 60Hz -> 16ms (slightly overshoots to avoid frame skips)
            let target_frame_duration_ms: u64 =
                if let Some(refresh_millihertz) = monitor.refresh_rate_millihertz() {
                    let refresh_hz = refresh_millihertz / 1000;
                    (1000 / refresh_hz) as u64
                } else {
                    16 // Default to 60 FPS if query fails
                };

            // Calculate window dimensions: height = min(width, height/2), width = height/2
            let window_height = screen_width.min(screen_height) / 2;
            let window_width = window_height / 2;
            let x = screen_width.min(screen_height) / 2 - window_width / 2;
            let y = screen_width.min(screen_height) / 2 - window_height / 2;

            let window_attributes = Window::default_attributes()
                .with_title("Photon")
                .with_inner_size(winit::dpi::PhysicalSize::new(window_width, window_height))
                .with_position(winit::dpi::PhysicalPosition::new(x, y))
                .with_decorations(false)
                .with_transparent(true);

            self.window = Some(event_loop.create_window(window_attributes).unwrap());

            if let Some(window) = &self.window {
                // Windows: Set up layered window for transparency
                #[cfg(target_os = "windows")]
                {
                    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    if let Ok(handle) = window.window_handle() {
                        if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                            unsafe {
                                enable_windows_transparency(win32_handle.hwnd.get() as _);
                            }
                        }
                    }
                }

                // Unified app creation for all desktop platforms
                let app = PhotonApp::new(
                    window,
                    self.blinkey_blink_rate_ms,
                    target_frame_duration_ms,
                    self.event_proxy.clone(),
                );
                self.photon_app = Some(app);
                window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let (Some(app), Some(window)) = (&mut self.photon_app, &self.window) {
                    // Learn maximized dimensions the first time is_maximized=true (reliable)
                    if window.is_maximized() && self.maximized_size.is_none() {
                        self.maximized_size = Some((size.width, size.height));
                    }

                    // Determine fullscreen state: match against known maximized size or query
                    let is_fullscreen = if let Some((max_w, max_h)) = self.maximized_size {
                        size.width == max_w && size.height == max_h
                    } else {
                        window.fullscreen().is_some()
                    };

                    app.set_fullscreen(is_fullscreen);
                    app.resize(size);
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(app) = &mut self.photon_app {
                    app.render();
                    // Animation timing is now handled in about_to_wait() via ControlFlow::WaitUntil
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                if let Some(app) = &mut self.photon_app {
                    app.update_modifiers(modifiers.state());
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let (Some(app), Some(window)) = (&mut self.photon_app, &self.window) {
                    if event.state.is_pressed() {
                        if let Some(text) = &event.text {
                            debug_println!("⌨️  KEYBOARD EVENT: key pressed, text={:?}", text);
                        }
                    }
                    app.handle_keyboard(event);
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let (Some(app), Some(window)) = (&mut self.photon_app, &self.window) {
                    app.handle_mouse_click(window, state, button);
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let (Some(window), Some(app)) = (&self.window, &mut self.photon_app) {
                    let needs_redraw = app.handle_mouse_move(window, position);
                    // Request redraw if mouse move handler says we need it (selection or hover changes)
                    if needs_redraw {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let (Some(window), Some(app)) = (&self.window, &mut self.photon_app) {
                    if app.handle_mouse_wheel(delta) {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let (Some(window), Some(app)) = (&self.window, &mut self.photon_app) {
                    app.handle_blinkey_left();
                    window.request_redraw();
                }
            }
            WindowEvent::HoveredFile(path) => {
                if let (Some(window), Some(app)) = (&self.window, &mut self.photon_app) {
                    app.handle_file_hover(&path);
                    if app.window_dirty {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::HoveredFileCancelled => {
                if let (Some(window), Some(app)) = (&self.window, &mut self.photon_app) {
                    app.handle_file_hover_cancelled();
                    if app.window_dirty {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::DroppedFile(path) => {
                if let (Some(window), Some(app)) = (&self.window, &mut self.photon_app) {
                    if let Err(e) = app.handle_dropped_file(&path) {
                        photon_messenger::log(&format!("Failed to load avatar: {}", e));
                    }
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(app) = &mut self.photon_app {
            use winit::event_loop::ControlFlow;

            // Priority 1: If selecting, use Poll mode and update scroll continuously
            if app.is_mouse_selecting {
                event_loop.set_control_flow(ControlFlow::Poll);
                // Only request redraw if scroll actually changed
                if app.update_selection_scroll() {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                return;
            }

            // Check for FGTW connectivity status (non-blocking)
            app.check_fgtw_online();
            if app.controls_dirty {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Check for attestation responses (non-blocking) - always check, regardless of focus
            if app.check_attestation_response() {
                // Attestation completed, redraw to update button
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Check for search results (non-blocking)
            if app.check_search_result() {
                // Search completed, redraw to show result
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Check for contact status updates (non-blocking)
            if app.check_status_updates() {
                // Contact status or CLUTCH state changed, full redraw needed
                app.window_dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Check for peer update notifications from FGTW WebSocket (non-blocking)
            if app.check_peer_updates() {
                // Peer IP changed, update cache and redraw
                app.window_dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Check for completed avatar downloads (non-blocking)
            if app.check_avatar_downloads() {
                // Avatar loaded, redraw to show it
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Check for completed CLUTCH keypair generation (non-blocking)
            if app.check_clutch_keygens() {
                // Keypairs ready, may have sent offer, redraw
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Check for completed CLUTCH KEM encapsulation (non-blocking)
            if app.check_clutch_kem_encaps() {
                // KEM response sent, redraw
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Check for completed CLUTCH ceremony (non-blocking)
            if app.check_clutch_ceremonies() {
                // Ceremony complete, chains created, redraw
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            // Periodically ping contacts to check online status
            app.maybe_ping_contacts();

            // Periodically refresh FGTW to keep port info fresh
            app.maybe_refresh_fgtw();

            // Check for refresh results and update contact IPs
            app.check_refresh_result();

            // Priority 2: If animating query, sync to display refresh rate
            if app.should_animate() {
                let now = std::time::Instant::now();
                if now >= app.next_animation_frame {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    // Advance to next frame immediately to avoid busy-looping
                    app.next_animation_frame =
                        now + std::time::Duration::from_millis(app.target_frame_duration_ms);
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(app.next_animation_frame));
                return;
            }

            // Priority 3: Handle blinkey and zoom hint timers
            let now = std::time::Instant::now();

            // Check zoom hint timer - request redraw if expired
            if app.zoom_hint_visible {
                if let Some(hide_time) = app.zoom_hint_hide_time {
                    if now >= hide_time {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }

            if app.textbox_is_focused() {
                // Check if it's time to blink
                if now >= app.next_blinkey_blink_time {
                    let font_size = app.font_size() as usize;
                    PhotonApp::flip_blinkey(
                        &mut app.renderer,
                        app.width as usize,
                        app.blinkey_pixel_x,
                        app.blinkey_pixel_y,
                        &mut app.blinkey_visible,
                        &mut app.blinkey_wave_top_bright,
                        font_size,
                        app.is_mouse_selecting,
                    );
                    app.next_blinkey_blink_time = app.next_blink_wake_time();
                }

                // Wake at earliest of blinkey time or zoom hint hide time
                let mut wake_time = app.next_blinkey_blink_time;
                if let Some(hide_time) = app.zoom_hint_hide_time {
                    if hide_time < wake_time {
                        wake_time = hide_time;
                    }
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(wake_time));
            } else {
                // No active textbox - poll every 250ms for network updates
                // But wake earlier if zoom hint needs to hide
                let mut wake_time = now + std::time::Duration::from_millis(250);
                if let Some(hide_time) = app.zoom_hint_hide_time {
                    if hide_time < wake_time {
                        wake_time = hide_time;
                    }
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(wake_time));
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: PhotonEvent) {
        match event {
            PhotonEvent::ConnectivityChanged(online) => {
                if let Some(app) = &mut self.photon_app {
                    if online != app.fgtw_online {
                        app.fgtw_online = online;
                        app.controls_dirty = true;
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }
            PhotonEvent::AttestationComplete => {
                // Wake up event loop to check attestation result
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            PhotonEvent::MessageReceived => {
                // Future: handle incoming messages
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            PhotonEvent::NetworkUpdate => {
                // Network data available (status, CLUTCH, avatar, etc.)
                // Just request a redraw to process the pending data
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            PhotonEvent::ClutchKeygenComplete => {
                // Background CLUTCH keypair generation finished
                // Request redraw to process the result
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            PhotonEvent::ClutchKemEncapComplete => {
                // Background CLUTCH KEM encapsulation finished
                // Request redraw to process the result
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            PhotonEvent::ClutchCeremonyComplete => {
                // Background CLUTCH ceremony completion (avalanche_expand) finished
                // Request redraw to process the result
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn enable_windows_transparency(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED,
    };

    let hwnd = HWND(hwnd as *mut _);

    // Set WS_EX_LAYERED style - REQUIRED for UpdateLayeredWindow
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
    let new_style = ex_style | WS_EX_LAYERED.0 as i32;
    SetWindowLongW(hwnd, GWL_EXSTYLE, new_style);

    // NOTE: Do NOT call SetLayeredWindowAttributes - it conflicts with UpdateLayeredWindow
    // UpdateLayeredWindow handles the alpha blending directly
}

/// Get the system blinkey blink rate in milliseconds
fn get_system_blinkey_blink_rate() -> u64 {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime;
        unsafe {
            let rate = GetCaretBlinkTime();
            if rate == 0 {
                // 0 means blinking is disabled, use default
                500
            } else {
                rate as u64
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Try to read from GNOME settings
        let blink_rate = std::process::Command::new("gsettings")
            .args(&["get", "org.gnome.desktop.interface", "blinkey-blink-time"])
            .output()
            .ok()
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
            })
            .unwrap_or(1200); // GNOME default is 1200ms for full cycle

        // Divide by 2 to get half-cycle (blink interval)
        blink_rate / 2
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        500 // Default fallback
    }
}

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

    // Set cursor size for Linux/X11 to match system cursor settings
    // Winit doesn't read the DE cursor size, so we need to set it manually
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

    let event_loop = EventLoop::<PhotonEvent>::with_user_event().build().unwrap();
    let event_proxy = event_loop.create_proxy();
    let blinkey_blink_rate = get_system_blinkey_blink_rate();
    let mut app = App {
        window: None,
        photon_app: None,
        screen_width: 0,
        screen_height: 0,
        maximized_size: None,
        blinkey_blink_rate_ms: blinkey_blink_rate,
        event_proxy,
    };

    event_loop.run_app(&mut app).unwrap();
}
