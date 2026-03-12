use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// Buffer guard — Deref gives you the DIB's actual pixel memory.
/// Compositor writes premultiplied pixels directly (PREMULTIPLIED=true on Windows).
pub struct WindowsBuffer<'a> {
    pixels: &'a mut [u32],
    renderer: &'a Renderer,
}

impl<'a> std::ops::Deref for WindowsBuffer<'a> {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        self.pixels
    }
}

impl<'a> std::ops::DerefMut for WindowsBuffer<'a> {
    fn deref_mut(&mut self) -> &mut [u32] {
        self.pixels
    }
}

impl<'a> WindowsBuffer<'a> {
    pub fn as_mut(&mut self) -> &mut [u32] {
        self.pixels
    }

    pub fn mark_rows(&self, _y_start: u32, _y_end: u32) {}
    pub fn mark_all(&self) {}

    pub fn present(self) -> Result<(), ()> {
        unsafe {
            let size = windows::Win32::Foundation::SIZE {
                cx: self.renderer.width as i32,
                cy: self.renderer.height as i32,
            };

            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let hdc_desktop = GetDC(None);

            let _ = UpdateLayeredWindow(
                self.renderer.hwnd,
                hdc_desktop,
                None,
                Some(&size),
                self.renderer.hdc_mem,
                Some(&POINT { x: 0, y: 0 }),
                None,
                Some(&blend),
                ULW_ALPHA,
            );
        }
        Ok(())
    }
}

pub struct Renderer {
    hwnd: HWND,
    width: u32,
    height: u32,
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

            let (hbitmap, bitmap_bits) = Self::create_dib(hdc_mem, width, height);
            SelectObject(hdc_mem, hbitmap);

            Self {
                hwnd,
                width,
                height,
                hdc_screen,
                hdc_mem,
                hbitmap,
                bitmap_bits,
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

    pub fn mark_rows(&mut self, _y_start: u32, _y_end: u32) {}
    pub fn mark_all(&mut self) {}

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            unsafe {
                DeleteObject(self.hbitmap);

                self.width = width;
                self.height = height;

                let (hbitmap, bits) = Self::create_dib(self.hdc_mem, width, height);
                self.hbitmap = hbitmap;
                self.bitmap_bits = bits;
                SelectObject(self.hdc_mem, self.hbitmap);
            }
        }
    }

    pub fn lock_buffer(&mut self) -> WindowsBuffer<'_> {
        let len = (self.width * self.height) as usize;
        let pixels = unsafe { std::slice::from_raw_parts_mut(self.bitmap_bits, len) };
        WindowsBuffer {
            pixels,
            renderer: self,
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
