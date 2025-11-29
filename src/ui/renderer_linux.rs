use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use winit::window::Window;

pub struct Renderer {
    context: Context<&'static Window>,
    surface: Surface<&'static Window, &'static Window>,
    width: u32,
    height: u32,
}

impl Renderer {
    pub fn new(window: &Window, width: u32, height: u32) -> Self {
        // SAFETY: We extend the lifetime to 'static because the surface will live
        // as long as the renderer, and the window is guaranteed to outlive both
        let static_window: &'static Window = unsafe { std::mem::transmute(window) };

        let context = Context::new(static_window).unwrap();
        let mut surface = Surface::new(&context, static_window).unwrap();

        surface
            .resize(
                NonZeroU32::new(width).unwrap(),
                NonZeroU32::new(height).unwrap(),
            )
            .unwrap();

        Self {
            context,
            surface,
            width,
            height,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            let _ = self.surface.resize(
                NonZeroU32::new(width).unwrap(),
                NonZeroU32::new(height).unwrap(),
            );
        }
    }

    /// Get mutable access to softbuffer's internal buffer for direct drawing
    /// Important: Call .present() on the returned buffer when done, don't drop it early!
    pub fn lock_buffer(&mut self) -> softbuffer::Buffer<'_, &'static Window, &'static Window> {
        self.surface.buffer_mut().unwrap()
    }
}
