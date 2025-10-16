use winit::window::Window;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, SelectObject, DeleteDC, DeleteObject,
    GetDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    HDC, HBITMAP, BLENDFUNCTION, AC_SRC_OVER, AC_SRC_ALPHA,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

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
            let hbitmap = CreateDIBSection(
                hdc_mem,
                &bmi,
                DIB_RGB_COLORS,
                &mut bitmap_bits,
                None,
                0,
            ).unwrap();

            SelectObject(hdc_mem, hbitmap);

            let pixel_buffer = vec![0u8; (width * height * 4) as usize];

            // Initialize bitmap to transparent black
            let bitmap_slice = std::slice::from_raw_parts_mut(
                bitmap_bits as *mut u32,
                (width * height) as usize,
            );
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
                self.pixel_buffer = vec![0u8; (width * height * 4) as usize];

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
                ).unwrap();

                SelectObject(self.hdc_mem, self.hbitmap);
                self.bitmap_bits = bitmap_bits as *mut u32;
            }
        }
    }

    pub fn get_pixel_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.pixel_buffer
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
                    bitmap_slice[i] = 0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
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
                None, // No color key
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
