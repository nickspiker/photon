use ndk::native_window::NativeWindow;
use ndk_sys::{ANativeWindow_Buffer, ANativeWindow_lock, ANativeWindow_unlockAndPost};

/// Samsung devices have a compositor that breaks magic pixel optimization
static mut SAMSUNG_MODE: bool = false;

/// Set Samsung mode (called once from JNI init before any rendering)
pub fn set_samsung_mode(is_samsung: bool) {
    unsafe { SAMSUNG_MODE = is_samsung; }
}

fn is_samsung() -> bool {
    unsafe { SAMSUNG_MODE }
}

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

    /// Current content version - incremented on each dirty frame
    content_version: u32,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            buffer: vec![0; (width * height) as usize],
            content_version: 1, // Start at 1 so 0 (uninitialized) never matches
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.buffer.resize((width * height) as usize, 0);
            // Force update on resize
            self.content_version = self.content_version.wrapping_add(1);
            if self.content_version == 0 { self.content_version = 1; }
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
    /// Uses magic pixel in top-right corner to track per-buffer state.
    /// Each of Android's 3 rotating buffers gets the content_version written
    /// to it after copy, so we can detect if a buffer is already current.
    pub fn present(&mut self, window: &NativeWindow, dirty: bool) -> bool {
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

            let src_width = self.width as usize;
            let copy_height = height.min(self.height as usize);
            let copy_width = width.min(src_width);

            // Increment content version when dirty
            if dirty {
                self.content_version = self.content_version.wrapping_add(1);
                if self.content_version == 0 { self.content_version = 1; }
            }

            let copied = if is_samsung() {
                // Samsung: always copy (their compositor breaks magic pixel)
                for y in 0..copy_height {
                    let src_row = &self.buffer[y * src_width..y * src_width + copy_width];
                    let dst_row = &mut dst_pixels[y * stride..y * stride + copy_width];
                    dst_row.copy_from_slice(src_row);
                }
                true
            } else {
                // Check magic pixel: does THIS buffer already have current content?
                let magic_idx = width.saturating_sub(1);
                let buffer_is_current = magic_idx < stride
                    && dst_pixels[magic_idx] == self.content_version;

                if buffer_is_current {
                    // This specific buffer already has the right content
                    false
                } else {
                    // Copy and stamp with current version
                    for y in 0..copy_height {
                        let src_row = &self.buffer[y * src_width..y * src_width + copy_width];
                        let dst_row = &mut dst_pixels[y * stride..y * stride + copy_width];
                        dst_row.copy_from_slice(src_row);
                    }
                    // Write magic pixel so we recognize this buffer next time
                    if magic_idx < stride {
                        dst_pixels[magic_idx] = self.content_version;
                    }
                    true
                }
            };

            // Always post - required for Choreographer timing
            ANativeWindow_unlockAndPost(window.ptr().as_ptr());
            copied
        }
    }

    /// Get buffer dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
