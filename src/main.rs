// Hide console window on Windows
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod crypto;
mod network;
mod storage;
mod types;
mod ui;

use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

struct App {
    window: Option<Window>,
    tmessage_app: Option<ui::TMessageApp>,
    screen_width: u32,
    screen_height: u32,
}

impl ApplicationHandler for App {
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

            // Calculate window dimensions: height = min(width, height/2), width = height/2
            let window_height = screen_width.min(screen_height) / 2;
            let window_width = window_height / 2;
            let x = screen_width.min(screen_height) / 2 - window_width / 2;
            let y = screen_width.min(screen_height) / 2 - window_height / 2;

            let window_attributes = Window::default_attributes()
                .with_title("tmessage")
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

                #[cfg(target_os = "windows")]
                {
                    self.tmessage_app = Some(ui::TMessageApp::new(
                        window,
                        self.screen_width,
                        self.screen_height,
                    ));
                }

                #[cfg(target_os = "linux")]
                {
                    self.tmessage_app = Some(pollster::block_on(ui::TMessageApp::new(
                        window,
                        self.screen_width,
                        self.screen_height,
                    )));
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(app) = &mut self.tmessage_app {
                    app.resize(size);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(app) = &mut self.tmessage_app {
                    app.render();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(app) = &mut self.tmessage_app {
                    app.handle_keyboard(event);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let (Some(app), Some(window)) = (&mut self.tmessage_app, &self.window) {
                    app.handle_mouse_click(window, state, button);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let (Some(window), Some(app)) = (&self.window, &mut self.tmessage_app) {
                    app.handle_mouse_move(window, position);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn enable_windows_transparency(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED};

    let hwnd = HWND(hwnd as *mut _);

    // Set WS_EX_LAYERED style - REQUIRED for UpdateLayeredWindow
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
    let new_style = ex_style | WS_EX_LAYERED.0 as i32;
    SetWindowLongW(hwnd, GWL_EXSTYLE, new_style);

    // NOTE: Do NOT call SetLayeredWindowAttributes - it conflicts with UpdateLayeredWindow
    // UpdateLayeredWindow handles the alpha blending directly
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();

    let event_loop = EventLoop::new().unwrap();
    let mut app = App {
        window: None,
        tmessage_app: None,
        screen_width: 0,  // Will be set in resumed()
        screen_height: 0, // Will be set in resumed()
    };

    event_loop.run_app(&mut app).unwrap();
}
