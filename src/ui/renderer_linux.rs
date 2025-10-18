use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use winit::window::Window;

pub struct Renderer {
    context: Context<&'static Window>,
    surface: Surface<&'static Window, &'static Window>,
    width: u32,
    height: u32,
    pixel_buffer: Vec<u8>,
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

        let mut pixel_buffer = Vec::with_capacity((width * height * 4) as usize);
        unsafe {
            pixel_buffer.set_len((width * height * 4) as usize);
        }

        Self {
            context,
            surface,
            width,
            height,
            pixel_buffer,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            let new_size = (width * height * 4) as usize;
            if new_size > self.pixel_buffer.capacity() {
                self.pixel_buffer
                    .reserve(new_size - self.pixel_buffer.len());
            }
            unsafe {
                self.pixel_buffer.set_len(new_size);
            }

            let _ = self.surface.resize(
                NonZeroU32::new(width).unwrap(),
                NonZeroU32::new(height).unwrap(),
            );
        }
    }

    pub fn get_pixel_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.pixel_buffer
    }

    pub fn present(&mut self) {
        let mut buffer = self.surface.buffer_mut().unwrap();

        // Convert RGBA to u32 pixel format
        for i in 0..(self.width * self.height) as usize {
            let idx = i * 4;
            let r = self.pixel_buffer[idx] as u32;
            let g = self.pixel_buffer[idx + 1] as u32;
            let b = self.pixel_buffer[idx + 2] as u32;
            let a = self.pixel_buffer[idx + 3] as u32;

            // Pack as ARGB or BGRA depending on platform
            // softbuffer handles endianness for us
            buffer[i] = (a << 24) | (r << 16) | (g << 8) | b;
        }

        buffer.present().unwrap();
    }
}
