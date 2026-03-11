use std::cell::Cell;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// Buffer guard that exposes the CPU pixel buffer as `&mut [u32]`.
/// Call `.present()` to copy dirty rows into the DIB and update the layered window.
pub struct WindowsBuffer<'a> {
    inner: &'a mut Renderer,
}

impl<'a> std::ops::Deref for WindowsBuffer<'a> {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        &self.inner.cpu_buffer
    }
}

impl<'a> std::ops::DerefMut for WindowsBuffer<'a> {
    fn deref_mut(&mut self) -> &mut [u32] {
        &mut self.inner.cpu_buffer
    }
}

impl<'a> WindowsBuffer<'a> {
    pub fn as_mut(&mut self) -> &mut [u32] {
        &mut self.inner.cpu_buffer
    }

    pub fn mark_rows(&self, _y_start: u32, _y_end: u32) {}
    pub fn mark_all(&self) {}

    pub fn present(self) -> Result<(), ()> {
        self.inner.present();
        Ok(())
    }
}

pub struct Renderer {
    hwnd: HWND,
    width: u32,
    height: u32,
    /// CPU pixel buffer — same 0xAARRGGBB format as all other platforms
    cpu_buffer: Vec<u32>,
    hdc_screen: HDC,
    hdc_mem: HDC,
    hbitmap: HBITMAP,
    bitmap_bits: *mut u32,
    dirty_y_min: Cell<u32>,
    dirty_y_max: Cell<u32>,
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

            let (hbitmap, bitmap_bits) = Self::create_dib(hdc_mem, width, height);
            SelectObject(hdc_mem, hbitmap);

            Self {
                hwnd,
                width,
                height,
                cpu_buffer: vec![0u32; (width * height) as usize],
                hdc_screen,
                hdc_mem,
                hbitmap,
                bitmap_bits,
                dirty_y_min: Cell::new(0),
                dirty_y_max: Cell::new(height),
            }
        }
    }

    unsafe fn create_dib(hdc: HDC, width: u32, height: u32) -> (HBITMAP, *mut u32) {
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

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbitmap =
            CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        (hbitmap, bits as *mut u32)
    }

    pub fn mark_rows(&mut self, y_start: u32, y_end: u32) {
        let y_end = y_end.min(self.height);
        self.dirty_y_min.set(self.dirty_y_min.get().min(y_start));
        self.dirty_y_max.set(self.dirty_y_max.get().max(y_end));
    }

    pub fn mark_all(&mut self) {
        self.dirty_y_min.set(0);
        self.dirty_y_max.set(self.height);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            unsafe {
                DeleteObject(self.hbitmap);

                self.width = width;
                self.height = height;
                self.cpu_buffer.resize((width * height) as usize, 0);

                let (hbitmap, bits) = Self::create_dib(self.hdc_mem, width, height);
                self.hbitmap = hbitmap;
                self.bitmap_bits = bits;
                SelectObject(self.hdc_mem, self.hbitmap);

                // Force full update after resize
                self.dirty_y_min.set(0);
                self.dirty_y_max.set(height);
            }
        }
    }

    pub fn lock_buffer(&mut self) -> WindowsBuffer<'_> {
        WindowsBuffer { inner: self }
    }

    pub fn present(&mut self) {
        let dy_min = self.dirty_y_min.get();
        let dy_max = self.dirty_y_max.get().min(self.height);

        // Reset dirty range for next frame
        self.dirty_y_min.set(u32::MAX);
        self.dirty_y_max.set(0);

        unsafe {
            if dy_min < dy_max {
                // Only premultiply dirty rows into the DIB
                let w = self.width as usize;
                let bitmap_slice = std::slice::from_raw_parts_mut(
                    self.bitmap_bits,
                    (self.width * self.height) as usize,
                );

                let start = dy_min as usize * w;
                let end = dy_max as usize * w;

                for i in start..end {
                    let pixel = self.cpu_buffer[i];
                    let a = (pixel >> 24) & 0xFF;

                    if a == 0 {
                        bitmap_slice[i] = 0;
                    } else if a == 255 {
                        bitmap_slice[i] = pixel;
                    } else {
                        let r = ((pixel >> 16) & 0xFF) * a / 255;
                        let g = ((pixel >> 8) & 0xFF) * a / 255;
                        let b = (pixel & 0xFF) * a / 255;
                        bitmap_slice[i] = (a << 24) | (r << 16) | (g << 8) | b;
                    }
                }
            }

            // UpdateLayeredWindow always reads the full DIB — non-dirty rows
            // are still correct from the previous frame
            let size = windows::Win32::Foundation::SIZE {
                cx: self.width as i32,
                cy: self.height as i32,
            };

            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let hdc_desktop = GetDC(None);

            let _ = UpdateLayeredWindow(
                self.hwnd,
                hdc_desktop,
                None,
                Some(&size),
                self.hdc_mem,
                Some(&POINT { x: 0, y: 0 }),
                None,
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
