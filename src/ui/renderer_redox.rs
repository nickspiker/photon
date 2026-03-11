use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use winit::window::Window;

/// Buffer wrapper that adds mark_rows/mark_all to softbuffer's Buffer.
/// On Redox (softbuffer), dirty tracking is a no-op — the compositor owns the buffer.
pub struct RedoxBuffer<'a> {
    inner: softbuffer::Buffer<'a, &'static Window, &'static Window>,
}

impl<'a> std::ops::Deref for RedoxBuffer<'a> {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        &self.inner
    }
}

impl<'a> std::ops::DerefMut for RedoxBuffer<'a> {
    fn deref_mut(&mut self) -> &mut [u32] {
        &mut self.inner
    }
}

impl<'a> RedoxBuffer<'a> {
    pub fn as_mut(&mut self) -> &mut [u32] {
        &mut self.inner
    }

    /// Mark a row range as dirty (no-op on Redox — softbuffer presents full buffer).
    pub fn mark_rows(&self, _y_start: u32, _y_end: u32) {}

    /// Mark the entire buffer as dirty (no-op on Redox).
    pub fn mark_all(&self) {}

    pub fn present(self) -> Result<(), ()> {
        self.inner.present().map_err(|_| ())
    }
}

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

    pub fn mark_rows(&mut self, _y_start: u32, _y_end: u32) {}
    pub fn mark_all(&mut self) {}

    /// Get mutable access to softbuffer's internal buffer for direct drawing
    pub fn lock_buffer(&mut self) -> RedoxBuffer<'_> {
        RedoxBuffer {
            inner: self.surface.buffer_mut().unwrap(),
        }
    }
}
