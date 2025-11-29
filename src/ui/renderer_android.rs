use ndk::native_window::NativeWindow;
use ndk_sys::{ANativeWindow_Buffer, ANativeWindow_lock, ANativeWindow_unlockAndPost};

/// Buffer wrapper for Android that mimics softbuffer's interface
/// Allows compositing code to work identically across platforms
pub struct AndroidBuffer<'a> {
    pixels: &'a mut Vec<u32>,
}

impl<'a> AndroidBuffer<'a> {
    pub fn as_mut(&mut self) -> &mut [u32] {
        self.pixels.as_mut_slice()
    }

    /// No-op present for Android - actual present happens via Renderer::present()
    /// This exists so compositing code's `buffer.present().unwrap()` compiles
    pub fn present(self) -> Result<(), ()> {
        Ok(())
    }
}

impl<'a> std::ops::Deref for AndroidBuffer<'a> {
    type Target = [u32];
    fn deref(&self) -> &Self::Target {
        self.pixels.as_slice()
    }
}

impl<'a> std::ops::DerefMut for AndroidBuffer<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.pixels.as_mut_slice()
    }
}

pub struct Renderer {
    width: u32,
    height: u32,

    /// Internal pixel buffer - compositing draws here
    buffer: Vec<u32>,

    /// Magic pixel counter for buffer tracking
    /// Android gives us random buffers from a pool, this tracks which one we last wrote to
    magic_counter: u32,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            buffer: vec![0; (width * height) as usize],
            magic_counter: 1, // Start at 1 so 0 (cleared buffer) is always a miss
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.buffer.resize((width * height) as usize, 0);
        }
    }

    /// Lock buffer for drawing - matches desktop interface
    /// Compositing code calls this to get a &mut [u32] to draw into
    pub fn lock_buffer(&mut self) -> AndroidBuffer<'_> {
        AndroidBuffer {
            pixels: &mut self.buffer,
        }
    }

    /// Present internal buffer to Android NativeWindow surface
    /// Only call this after compositing has drawn to lock_buffer()
    ///
    /// Returns true if pixels were copied, false if magic pixel matched (no copy needed)
    pub fn present(&mut self, window: &NativeWindow, dirty: bool) -> bool {
        // Early out if nothing is dirty
        if !dirty {
            return false;
        }

        unsafe {
            let mut android_buffer = std::mem::zeroed::<ANativeWindow_Buffer>();

            if ANativeWindow_lock(
                window.ptr().as_ptr(),
                &mut android_buffer,
                std::ptr::null_mut(),
            ) < 0
            {
                log::error!("Failed to lock NativeWindow buffer");
                return false;
            }

            let stride = android_buffer.stride as usize;
            let height = android_buffer.height as usize;
            let width = android_buffer.width as usize;

            // RGBA_8888: 4 bytes per pixel, interpret as u32
            let dst_pixels =
                std::slice::from_raw_parts_mut(android_buffer.bits as *mut u32, stride * height);

            // Check magic pixel at top-right corner (stride - 1, not width - 1)
            let magic_idx = stride - 1;
            let needs_full_copy = dst_pixels[magic_idx] != self.magic_counter;

            if needs_full_copy {
                // Increment magic counter for next frame
                self.magic_counter = self.magic_counter.wrapping_add(1);
                if self.magic_counter == 0 {
                    self.magic_counter = 1; // Skip 0
                }

                // Direct copy - colours already in ABGR format via theme::fmt()
                let src_width = self.width as usize;
                let copy_height = height.min(self.height as usize);
                let copy_width = width.min(src_width);

                for y in 0..copy_height {
                    let src_row = &self.buffer[y * src_width..y * src_width + copy_width];
                    let dst_row = &mut dst_pixels[y * stride..y * stride + copy_width];
                    dst_row.copy_from_slice(src_row);
                }

                // Write magic pixel
                dst_pixels[magic_idx] = self.magic_counter;
            }
            // If magic matched, we could do differential copy here for dirty regions
            // For now, full copy on any dirty (Android buffer recycling makes partial tricky)

            ANativeWindow_unlockAndPost(window.ptr().as_ptr());
            needs_full_copy
        }
    }

    /// Get buffer dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
