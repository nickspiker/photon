use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use winit::window::Window;

/// A pre-rendered screen page with associated metadata
pub struct ScreenPage {
    pub pixels: Vec<u32>,    // Pre-rendered screen in u32 ARGB format
    pub hit_map: Vec<u8>,    // Hit testing map (button IDs, etc.)
    pub text_mask: Vec<u8>,  // Text rendering alpha mask (0-255)
}

impl ScreenPage {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            pixels: vec![0; width * height],
            hit_map: vec![0; width * height],
            text_mask: vec![0; width * height],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.pixels.resize(width * height, 0);
        self.hit_map.resize(width * height, 0);
        self.text_mask.resize(width * height, 0);
    }
}

pub struct Renderer {
    context: Context<&'static Window>,
    surface: Surface<&'static Window, &'static Window>,
    width: u32,
    height: u32,

    // Screen pages for different screens
    pub login_page: ScreenPage,
    // Could add more pages: main_page, settings_page, etc.
}

impl Renderer {
    pub async fn new(window: &Window, width: u32, height: u32) -> Self {
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
            login_page: ScreenPage::new(width as usize, height as usize),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;

            // Resize all pages
            self.login_page.resize(width as usize, height as usize);

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

    /// Lock buffer, draw with callback, and present atomically
    pub fn draw_and_present<F>(&mut self, draw_fn: F)
    where
        F: FnOnce(&mut [u32]),
    {
        let mut buffer = self.surface.buffer_mut().unwrap();
        draw_fn(&mut buffer);
        buffer.present().unwrap();
    }

    /// Copy a pre-rendered page to the live buffer and present
    pub fn swap_to_page(&mut self, page_pixels: &[u32]) {
        let mut buffer = self.lock_buffer();
        buffer.copy_from_slice(page_pixels);
        buffer.present().unwrap();
    }
}
