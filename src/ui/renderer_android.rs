use ndk::native_window::NativeWindow;
use ndk_sys::{ANativeWindow_Buffer, ANativeWindow_lock, ANativeWindow_unlockAndPost};

/// Samsung devices have a compositor that breaks magic pixel optimization
/// Set once at startup, never changes
static mut SAMSUNG_MODE: bool = false;

/// Set Samsung mode (called once from JNI init before any rendering)
pub fn set_samsung_mode(is_samsung: bool) {
    // SAFETY: Called once at startup before any rendering threads
    unsafe { SAMSUNG_MODE = is_samsung; }
}

fn is_samsung() -> bool {
    // SAFETY: Read-only after init, no data race possible
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

    /// Magic pixel counter for non-Samsung devices
    /// Incremented on dirty frames, written to top-right pixel to detect stale buffers
    magic_counter: u32,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            buffer: vec![0; (width * height) as usize],
            magic_counter: 1, // Start at 1 so 0 (uninitialized) never matches
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.buffer.resize((width * height) as usize, 0);
            // Force full redraw after resize
            self.magic_counter = self.magic_counter.wrapping_add(1);
            if self.magic_counter == 0 {
                self.magic_counter = 1;
            }
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
    /// Always locks and posts every frame (Samsung requires this).
    /// Samsung: always copies (their compositor breaks magic pixel optimization)
    /// Non-Samsung: uses magic pixel in top-right to skip copy when buffer is current
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

            let copied = if is_samsung() {
                // Samsung: always copy everything, their compositor is weird
                for y in 0..copy_height {
                    let src_row = &self.buffer[y * src_width..y * src_width + copy_width];
                    let dst_row = &mut dst_pixels[y * stride..y * stride + copy_width];
                    dst_row.copy_from_slice(src_row);
                }
                true
            } else {
                // Non-Samsung: use magic pixel optimization
                let magic_idx = width.saturating_sub(1);
                let buffer_is_current = !dirty
                    && magic_idx < stride
                    && dst_pixels[magic_idx] == self.magic_counter;

                if buffer_is_current {
                    // Buffer already has correct content - skip the copy
                    false
                } else {
                    // Need to copy - either dirty or buffer is stale
                    if dirty {
                        self.magic_counter = self.magic_counter.wrapping_add(1);
                        if self.magic_counter == 0 {
                            self.magic_counter = 1;
                        }
                    }

                    for y in 0..copy_height {
                        let src_row = &self.buffer[y * src_width..y * src_width + copy_width];
                        let dst_row = &mut dst_pixels[y * stride..y * stride + copy_width];
                        dst_row.copy_from_slice(src_row);
                    }

                    // Write magic pixel
                    if magic_idx < stride {
                        dst_pixels[magic_idx] = self.magic_counter;
                    }
                    true
                }
            };

            // Always post - Samsung throttles Choreographer if we don't
            ANativeWindow_unlockAndPost(window.ptr().as_ptr());
            copied
        }
    }

    /// Get buffer dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
