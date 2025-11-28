//! JNI bindings for Android
//!
//! This module provides the native interface for the Android app.
//! The Kotlin/Java activity calls these functions to initialize and draw the UI.

#[cfg(target_os = "android")]
use jni::{
    objects::{JByteArray, JClass, JObject, JString},
    sys::{jboolean, jfloat, jint, jlong, JNI_FALSE, JNI_TRUE},
    JNIEnv,
};

#[cfg(target_os = "android")]
use log::*;

#[cfg(target_os = "android")]
use ndk::native_window::NativeWindow;

#[cfg(target_os = "android")]
use crate::ui::PhotonApp;

#[cfg(target_os = "android")]
use crate::network::fgtw::storage::Keypair;

/// Android-specific context wrapping PhotonApp with device keypair
#[cfg(target_os = "android")]
pub struct PhotonContext {
    pub app: PhotonApp,
    pub device_keypair: Keypair,
}

/// Derive device keypair from fingerprint bytes using BLAKE3
#[cfg(target_os = "android")]
fn derive_device_keypair(fingerprint: &[u8]) -> Keypair {
    use ed25519_dalek::SigningKey;

    // BLAKE3 hash the fingerprint to get 32 bytes for Ed25519 seed
    let hash = blake3::hash(fingerprint);
    let seed: [u8; 32] = *hash.as_bytes();

    let secret = SigningKey::from_bytes(&seed);
    let public = secret.verifying_key();

    Keypair { secret, public }
}

#[cfg(target_os = "android")]
impl PhotonContext {
    pub fn new(width: u32, height: u32, device_keypair: Keypair) -> Self {
        info!(
            "Device pubkey: {}",
            hex::encode(device_keypair.public.as_bytes())
        );
        Self {
            app: PhotonApp::new(width, height),
            device_keypair,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.app.width = width;
        self.app.height = height;
        let w = width as usize;
        let h = height as usize;
        self.app.min_dim = w.min(h);
        self.app.perimeter = w + h;
        self.app.diagonal_sq = w * w + h * h;
        self.app.renderer.resize(width, height);
        self.app.hit_test_map.resize((width * height) as usize, 0);
        self.app.textbox_mask.resize((width * height) as usize, 0);
        self.app.window_dirty = true;
    }

    pub fn draw(&mut self, window: &NativeWindow) {
        // Check dirty BEFORE render (render clears the flags)
        let mut dirty = self.app.window_dirty || self.app.text_dirty || self.app.selection_dirty;

        // Use the full PhotonApp render loop
        self.app.render();

        // Handle blinkey blinking (cursor animation)
        let now = std::time::Instant::now();
        if now >= self.app.next_blinkey_blink_time
            && self.app.current_text_state.textbox_focused
            && self.app.blinkey_visible
        {
            let width = self.app.width as usize;
            let blinkey_x = self.app.blinkey_pixel_x;
            let blinkey_y = self.app.blinkey_pixel_y;
            let font_size = self.app.font_size() as usize;
            let is_selecting = self.app.is_mouse_selecting;

            PhotonApp::flip_blinkey(
                &mut self.app.renderer,
                width,
                blinkey_x,
                blinkey_y,
                &mut self.app.blinkey_visible,
                &mut self.app.blinkey_wave_top_bright,
                font_size,
                is_selecting,
            );
            self.app.next_blinkey_blink_time = self.app.next_blink_wake_time();
            dirty = true; // Blinkey changed, need to present
        }

        // Present internal buffer to NativeWindow
        self.app.renderer.present(window, dirty);
    }
}

#[cfg(target_os = "android")]
fn get_context(ptr: jlong) -> Option<&'static mut PhotonContext> {
    if ptr == 0 {
        error!("Null PhotonContext pointer received");
        return None;
    }
    unsafe { Some(&mut *(ptr as *mut PhotonContext)) }
}

/// Initialize Photon UI with device fingerprint for key derivation
/// Returns a pointer to the PhotonContext
///
/// # Arguments
/// * `fingerprint` - Raw bytes from DeviceFingerprint.gather() containing
///   concatenated device identifiers (ANDROID_ID, Build.*, etc.)
///   This is hashed with BLAKE3 to derive the device's Ed25519 keypair.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeInit(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    width: jint,
    height: jint,
    fingerprint: JByteArray<'_>,
) -> jlong {
    info!("Initializing Photon: {}x{}", width, height);

    // Extract fingerprint bytes from JNI array
    let fingerprint_bytes = match env.convert_byte_array(&fingerprint) {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to read fingerprint bytes: {:?}", e);
            return 0;
        }
    };

    info!("Device fingerprint: {} bytes", fingerprint_bytes.len());

    // Derive device keypair from fingerprint
    let device_keypair = derive_device_keypair(&fingerprint_bytes);

    let context = Box::new(PhotonContext::new(width as u32, height as u32, device_keypair));
    let ptr = Box::into_raw(context) as jlong;

    info!("PhotonContext created at 0x{:x}", ptr as u64);
    ptr
}

/// Draw a frame to the surface
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeDraw(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    surface: JObject<'_>,
) {
    let Some(context) = get_context(context_ptr) else {
        error!("Invalid context pointer in nativeDraw");
        return;
    };

    // Convert Surface to NativeWindow
    let Some(window) = (unsafe { NativeWindow::from_surface(env.get_raw(), surface.as_raw()) })
    else {
        error!("Failed to convert Surface to NativeWindow");
        return;
    };

    context.draw(&window);
}

/// Handle resize
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeResize(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    width: jint,
    height: jint,
) {
    let Some(context) = get_context(context_ptr) else {
        error!("Invalid context pointer in nativeResize");
        return;
    };

    info!("Resizing Photon: {}x{}", width, height);
    context.resize(width as u32, height as u32);
}

/// Handle touch events
/// action: 0=DOWN, 1=UP, 2=MOVE, 3=CANCEL
/// Returns: 1=show keyboard, -1=hide keyboard, 0=no change
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeOnTouch(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    action: jint,
    x: jfloat,
    y: jfloat,
) -> jint {
    let Some(context) = get_context(context_ptr) else {
        return 0;
    };
    context.app.handle_touch(action, x, y)
}

/// Handle text input from soft keyboard
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeOnTextInput(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    text: JString<'_>,
) {
    let Some(context) = get_context(context_ptr) else {
        return;
    };

    let text_str: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(_) => return,
    };

    context.app.handle_text_input(&text_str);
}

/// Handle special key events (backspace, enter, arrows)
/// Returns true if the key was handled
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeOnKeyEvent(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    key_code: jint,
) -> jboolean {
    let Some(context) = get_context(context_ptr) else {
        return JNI_FALSE;
    };

    // Android KeyEvent codes
    const KEYCODE_DEL: i32 = 67;        // Backspace
    const KEYCODE_ENTER: i32 = 66;      // Enter
    const KEYCODE_DPAD_LEFT: i32 = 21;
    const KEYCODE_DPAD_RIGHT: i32 = 22;

    let handled = match key_code {
        KEYCODE_DEL => context.app.handle_backspace(),
        KEYCODE_ENTER => context.app.handle_enter(),
        KEYCODE_DPAD_LEFT => context.app.handle_arrow_left(),
        KEYCODE_DPAD_RIGHT => context.app.handle_arrow_right(),
        _ => false,
    };

    if handled { JNI_TRUE } else { JNI_FALSE }
}

/// Cleanup
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeDestroy(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
) {
    if context_ptr != 0 {
        info!("Destroying PhotonContext");
        unsafe {
            let _ = Box::from_raw(context_ptr as *mut PhotonContext);
        }
    }
}
