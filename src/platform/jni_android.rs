//! JNI bindings for Android
//!
//! This module provides the native interface for the Android app.
//! The Kotlin/Java activity calls these functions to initialize and draw the UI.

use crate::network::fgtw::FgtwTransport;
use crate::network::HandleQuery;
use crate::types::DevicePubkey;

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
use crate::ui::app::{AppState, LaunchState};
#[cfg(target_os = "android")]
use crate::ui::PhotonApp;

#[cfg(target_os = "android")]
use crate::network::fgtw::Keypair;

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
        use std::sync::Arc;

        info!(
            "Device pubkey: {}",
            hex::encode(device_keypair.public.as_bytes())
        );

        // Create app with keypair (keypair is stored in PhotonApp now)
        let mut app = PhotonApp::new(width, height, device_keypair.clone());

        // Initialize network stack with unified HandleQuery
        let handle_query = HandleQuery::new(device_keypair.clone());
        let our_identity = DevicePubkey::from_bytes(*device_keypair.public.as_bytes());
        let transport = Arc::new(FgtwTransport::new(our_identity, 41641));
        handle_query.set_transport(transport);
        app.set_handle_query(handle_query);

        Self {
            app,
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
        // Check for network updates using unified functions
        self.app.check_fgtw_online();
        self.app.check_attestation_response();
        self.app.check_search_result();

        // Check for FCM peer update poke - triggers immediate FGTW refresh
        if check_fcm_peer_update() {
            info!("FCM poke received - triggering FGTW refresh");
            self.app.force_fgtw_refresh();
        }

        // P2P status checking and FGTW refresh (unified with desktop)
        if self.app.check_status_updates() {
            self.app.window_dirty = true;
        }
        if self.app.check_avatar_downloads() {
            self.app.window_dirty = true;
        }
        self.app.maybe_ping_contacts();
        self.app.maybe_refresh_fgtw();
        self.app.check_refresh_result();

        // Check dirty BEFORE render to decide if we need to present
        // BUT: animation sets window_dirty during render for NEXT frame
        // So we check both before AND after
        let dirty_before = self.app.window_dirty
            || self.app.text_dirty
            || self.app.selection_dirty
            || self.app.controls_dirty;

        // Check if we're in an animating state BEFORE render clears window_dirty
        let is_animating = matches!(
            self.app.app_state,
            AppState::Launch(LaunchState::Attesting) | AppState::Searching
        );

        // Use the full PhotonApp render loop
        self.app.render();

        // Animation state always needs fresh buffer (render() clears window_dirty at end)
        let mut dirty = dirty_before || self.app.window_dirty || is_animating;

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
/// * `data_dir` - Android app's filesDir path for persistent storage
/// * `is_samsung` - True if running on Samsung device (needs Choreographer workarounds)
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeInit(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    width: jint,
    height: jint,
    fingerprint: JByteArray<'_>,
    data_dir: JString<'_>,
    is_samsung: jboolean,
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

    // Extract data directory path
    let data_dir_str: String = match env.get_string(&data_dir) {
        Ok(s) => s.into(),
        Err(e) => {
            error!("Failed to read data_dir: {:?}", e);
            return 0;
        }
    };

    let is_samsung = is_samsung != JNI_FALSE;
    info!("Device fingerprint: {} bytes", fingerprint_bytes.len());
    info!("Data directory: {}", data_dir_str);
    info!("Samsung device: {}", is_samsung);

    // Set global Android data directory for avatar storage
    crate::avatar::set_android_data_dir(data_dir_str);

    // Samsung's compositor breaks magic pixel optimization - always copy on Samsung
    crate::ui::renderer_android::set_samsung_mode(is_samsung);

    // Derive device keypair from fingerprint
    let device_keypair = derive_device_keypair(&fingerprint_bytes);

    let context = Box::new(PhotonContext::new(
        width as u32,
        height as u32,
        device_keypair,
    ));
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
    const KEYCODE_DEL: i32 = 67; // Backspace
    const KEYCODE_ENTER: i32 = 66; // Enter
    const KEYCODE_DPAD_LEFT: i32 = 21;
    const KEYCODE_DPAD_RIGHT: i32 = 22;

    let handled = match key_code {
        KEYCODE_DEL => context.app.handle_backspace(),
        KEYCODE_ENTER => context.app.handle_enter(),
        KEYCODE_DPAD_LEFT => context.app.handle_arrow_left(),
        KEYCODE_DPAD_RIGHT => context.app.handle_arrow_right(),
        _ => false,
    };

    if handled {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Handle Android back button
/// Returns true if handled (stay in app), false to exit
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeOnBackPressed(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
) -> jboolean {
    let Some(context) = get_context(context_ptr) else {
        return JNI_FALSE;
    };

    if context.app.handle_back() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Handle avatar file from image picker
/// Receives raw file bytes (JPEG/PNG/WebP) for proper ICC profile color management
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeSetAvatarFromFile(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    file_bytes: JByteArray<'_>,
) {
    let Some(context) = get_context(context_ptr) else {
        error!("Invalid context pointer in nativeSetAvatarFromFile");
        return;
    };

    let bytes = match env.convert_byte_array(&file_bytes) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to read file bytes: {:?}", e);
            return;
        }
    };

    info!("Received avatar file: {} bytes", bytes.len());
    context.app.set_avatar_from_file(bytes);
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

// ============================================================================
// FCM Push Notification Support
// ============================================================================

use std::sync::atomic::{AtomicBool, Ordering};

/// Flag set by FCM service when peer update received - triggers FGTW refresh
static FCM_PEER_UPDATE_PENDING: AtomicBool = AtomicBool::new(false);

/// Check and clear the FCM peer update flag
#[cfg(target_os = "android")]
pub fn check_fcm_peer_update() -> bool {
    FCM_PEER_UPDATE_PENDING.swap(false, Ordering::SeqCst)
}

/// Called from FirebaseMessagingService when peer_update FCM message received
/// This is called from a background thread, so we just set a flag
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonMessagingService_nativePeerUpdateReceived(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) {
    info!("FCM peer update received - flagging for refresh");
    FCM_PEER_UPDATE_PENDING.store(true, Ordering::SeqCst);
}
