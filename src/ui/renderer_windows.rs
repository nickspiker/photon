use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// Wrapper to mimic softbuffer::Buffer API for Windows
pub struct WindowsBuffer<'a> {
    renderer: &'a mut Renderer,
    u32_view: Vec<u32>,
}

impl<'a> WindowsBuffer<'a> {
    fn new(renderer: &'a mut Renderer) -> Self {
        // Create a u32 view of the pixel buffer
        let mut u32_view = Vec::with_capacity((renderer.width * renderer.height) as usize);
        for i in 0..(renderer.width * renderer.height) as usize {
            let idx = i * 4;
            let r = renderer.pixel_buffer[idx];
            let g = renderer.pixel_buffer[idx + 1];
            let b = renderer.pixel_buffer[idx + 2];
            let a = renderer.pixel_buffer[idx + 3];
            // ARGB format
            let pixel = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            u32_view.push(pixel);
        }

        Self { renderer, u32_view }
    }

    pub fn as_mut(&mut self) -> &mut [u32] {
        &mut self.u32_view
    }

    pub fn present(self) -> Result<(), ()> {
        // Convert u32 view back to u8 pixel buffer
        for i in 0..(self.renderer.width * self.renderer.height) as usize {
            let pixel = self.u32_view[i];
            let idx = i * 4;
            self.renderer.pixel_buffer[idx] = ((pixel >> 16) & 0xFF) as u8; // R
            self.renderer.pixel_buffer[idx + 1] = ((pixel >> 8) & 0xFF) as u8; // G
            self.renderer.pixel_buffer[idx + 2] = (pixel & 0xFF) as u8; // B
            self.renderer.pixel_buffer[idx + 3] = ((pixel >> 24) & 0xFF) as u8; // A
        }

        // Now call the actual Windows present to display on screen
        self.renderer.present();
        Ok(())
    }
}

pub struct Renderer {
    hwnd: HWND,
    width: u32,
    height: u32,
    pixel_buffer: Vec<u8>,
    hdc_screen: HDC,
    hdc_mem: HDC,
    hbitmap: HBITMAP,
    bitmap_bits: *mut u32,
}

impl Renderer {
    pub fn new(window: &Window, width: u32, height: u32) -> Self {
        let hwnd = match window.window_handle().unwrap().as_raw() {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
            _ => panic!("Not a Win32 window"),
        };

        unsafe {
            let hdc_screen = GetDC(hwnd);
            let hdc_mem = CreateCompatibleDC(hdc_screen);

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32), // Top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut bitmap_bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let hbitmap =
                CreateDIBSection(hdc_mem, &bmi, DIB_RGB_COLORS, &mut bitmap_bits, None, 0).unwrap();

            SelectObject(hdc_mem, hbitmap);

            let mut pixel_buffer = Vec::with_capacity((width * height * 4) as usize);
            unsafe {
                pixel_buffer.set_len((width * height * 4) as usize);
            }

            // Initialize bitmap to transparent black
            let bitmap_slice =
                std::slice::from_raw_parts_mut(bitmap_bits as *mut u32, (width * height) as usize);
            for pixel in bitmap_slice.iter_mut() {
                *pixel = 0x00000000; // Transparent black
            }

            Self {
                hwnd,
                width,
                height,
                pixel_buffer,
                hdc_screen,
                hdc_mem,
                hbitmap,
                bitmap_bits: bitmap_bits as *mut u32,
            }
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            unsafe {
                // Clean up old bitmap
                DeleteObject(self.hbitmap);

                self.width = width;
                self.height = height;
                let new_size = (width * height * 4) as usize;
                if new_size > self.pixel_buffer.capacity() {
                    self.pixel_buffer
                        .reserve(new_size - self.pixel_buffer.len());
                }
                self.pixel_buffer.set_len(new_size);

                // Create new bitmap
                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width as i32,
                        biHeight: -(height as i32),
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };

                let mut bitmap_bits: *mut std::ffi::c_void = std::ptr::null_mut();
                self.hbitmap = CreateDIBSection(
                    self.hdc_mem,
                    &bmi,
                    DIB_RGB_COLORS,
                    &mut bitmap_bits,
                    None,
                    0,
                )
                .unwrap();

                SelectObject(self.hdc_mem, self.hbitmap);
                self.bitmap_bits = bitmap_bits as *mut u32;
            }
        }
    }

    pub fn get_pixel_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.pixel_buffer
    }

    /// Get a buffer wrapper that mimics softbuffer::Buffer API
    pub fn lock_buffer(&mut self) -> WindowsBuffer<'_> {
        WindowsBuffer::new(self)
    }

    pub fn present(&mut self) {
        unsafe {
            // Copy pixel buffer to bitmap with pre-multiplied alpha
            let bitmap_slice = std::slice::from_raw_parts_mut(
                self.bitmap_bits,
                (self.width * self.height) as usize,
            );

            for i in 0..(self.width * self.height) as usize {
                let idx = i * 4;
                let r = self.pixel_buffer[idx];
                let g = self.pixel_buffer[idx + 1];
                let b = self.pixel_buffer[idx + 2];
                let a = self.pixel_buffer[idx + 3];

                if a == 0 {
                    bitmap_slice[i] = 0;
                } else if a == 255 {
                    // Opaque: ARGB format, no pre-multiply needed
                    bitmap_slice[i] =
                        0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
                } else {
                    // Semi-transparent: Pre-multiply RGB by alpha
                    let alpha_f = a as f32 / 255.0;
                    let r_pm = (r as f32 * alpha_f) as u32;
                    let g_pm = (g as f32 * alpha_f) as u32;
                    let b_pm = (b as f32 * alpha_f) as u32;
                    bitmap_slice[i] = ((a as u32) << 24) | (r_pm << 16) | (g_pm << 8) | b_pm;
                }
            }

            // Update layered window
            let size = windows::Win32::Foundation::SIZE {
                cx: self.width as i32,
                cy: self.height as i32,
            };

            let pt_src = POINT { x: 0, y: 0 };

            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            // UpdateLayeredWindow with screen DC
            let hdc_desktop = GetDC(None);

            let _ = UpdateLayeredWindow(
                self.hwnd,
                hdc_desktop,
                None, // Don't change position
                Some(&size),
                self.hdc_mem,
                Some(&pt_src),
                None, // No colour key
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.hbitmap);
            DeleteDC(self.hdc_mem);
        }
    }
}
