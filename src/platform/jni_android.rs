//! JNI bindings for Android.
//!
//! Two consumer surfaces hit these symbols:
//! - `PhotonActivity` (the foreground UI) → `Java_com_photon_messenger_PhotonActivity_native*`.
//!   These are thin shims into `fluor::host::android::AndroidShell<PhotonApp>` — the shell owns the surface + render pipeline + event translation, photon owns only the FluorApp impl and the app-specific bits (avatar picker, FCM peer-update flag).
//! - `PhotonConnectionService` (the background network stack) →
//!   `Java_com_photon_messenger_PhotonConnectionService_nativeNetwork*` + FCM peer-update notification. These survive the UI Activity's lifecycle so the persistent peer store + device keypair don't churn on each rotation / background trip.

use crate::network::fgtw::PeerStore;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "android")]
use jni::{
    objects::{JByteArray, JClass, JString},
    sys::{jboolean, jfloat, jint, jlong, JNI_FALSE, JNI_TRUE},
    JNIEnv,
};

#[cfg(target_os = "android")]
use log::*;

#[cfg(target_os = "android")]
use ndk::native_window::NativeWindow;

#[cfg(target_os = "android")]
use jni::objects::JObject;

#[cfg(target_os = "android")]
use crate::network::fgtw::Keypair;

#[cfg(target_os = "android")]
use crate::ui::PhotonApp;

#[cfg(target_os = "android")]
use fluor::host::android::AndroidShell;

// ============================================================================

// PhotonActivity context — wraps fluor::AndroidShell<PhotonApp> ============================================================================
/// Activity-side context. Holds the fluor shell that owns the FluorApp + surface + pipeline. Lifetime: created on Activity surface-creation (`nativeInitWithNetwork`), destroyed on Activity teardown (`nativeDestroy`).
#[cfg(target_os = "android")]
pub struct PhotonContext {
    pub shell: AndroidShell<PhotonApp>,
}

#[cfg(target_os = "android")]
impl PhotonContext {
    pub fn new(width: u32, height: u32, network: &NetworkContext) -> Self {
        // Inject the NetworkContext-derived device keypair BEFORE AndroidShell::new calls app.init — PhotonApp::init takes the keypair via `device_keypair.take()` and would panic on Android if it found `None`. The cryptographic identity for every contact / message / chain advance flows from this keypair, so the safety check is load-bearing.
        let mut app = PhotonApp::new();
        app.set_device_keypair(network.keypair.clone());
        info!(
            "PhotonContext: wired device keypair pubkey {}",
            hex::encode(network.keypair.public.as_bytes())
        );
        Self {
            shell: AndroidShell::new(app, width, height),
        }
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

/// Initialize the activity-side context. `network_ptr` is the service-owned `NetworkContext` pointer; `is_samsung` selects the surface's magic-pixel-cache fallback.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeInitWithNetwork(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    width: jint,
    height: jint,
    network_ptr: jlong,
    is_samsung: jboolean,
) -> jlong {
    info!(
        "PhotonActivity: nativeInitWithNetwork {}x{} (network @ 0x{:x}, samsung={})",
        width,
        height,
        network_ptr as u64,
        is_samsung != JNI_FALSE
    );
    if network_ptr == 0 {
        error!("Null NetworkContext pointer");
        return 0;
    }
    fluor::host::android::surface::set_samsung_mode(is_samsung != JNI_FALSE);
    let network = unsafe { &*(network_ptr as *const NetworkContext) };
    let context = Box::new(PhotonContext::new(width as u32, height as u32, network));
    Box::into_raw(context) as jlong
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeDraw(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    surface: JObject<'_>,
) {
    let Some(ctx) = get_context(context_ptr) else {
        return;
    };
    let Some(window) = (unsafe { NativeWindow::from_surface(env.get_raw(), surface.as_raw()) })
    else {
        error!("Failed to convert Surface to NativeWindow");
        return;
    };
    ctx.shell.draw(&window);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeResize(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    width: jint,
    height: jint,
) {
    let Some(ctx) = get_context(context_ptr) else {
        return;
    };
    info!("nativeResize {}x{}", width, height);
    ctx.shell.resize(width as u32, height as u32);
}

/// Returns: 1=show keyboard, -1=hide keyboard, 0=no change. AndroidShell::on_touch reads PhotonApp::wants_keyboard after dispatching the touch through the widget tree; the JNI shim just forwards the int.
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
    let Some(ctx) = get_context(context_ptr) else {
        return 0;
    };
    ctx.shell.on_touch(action, x, y) as jint
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeOnTextInput(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    text: JString<'_>,
) {
    let Some(ctx) = get_context(context_ptr) else {
        return;
    };
    let text_str: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(e) => {
            error!("Failed to read text input: {:?}", e);
            return;
        }
    };
    ctx.shell.on_text_input(text_str);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeOnKeyEvent(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    key_code: jint,
) -> jboolean {
    let Some(ctx) = get_context(context_ptr) else {
        return JNI_FALSE;
    };
    if ctx.shell.on_key_event(key_code) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeOnBackPressed(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
) -> jboolean {
    let Some(ctx) = get_context(context_ptr) else {
        return JNI_FALSE;
    };
    if ctx.shell.on_back_pressed() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeOnScale(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    scale_factor: jfloat,
) {
    let Some(ctx) = get_context(context_ptr) else {
        return;
    };
    ctx.shell.on_scale(scale_factor);
}

/// Avatar from image picker. NOT in AndroidShell — photon-specific (decodes via the existing avatar pipeline). Stubbed for now; wires through once PhotonApp exposes a `set_avatar_from_file(bytes)` method that funnels into the avatar storage layer.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeSetAvatarFromFile(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    file_bytes: JByteArray<'_>,
) {
    let Some(_ctx) = get_context(context_ptr) else {
        return;
    };
    let bytes = match env.convert_byte_array(&file_bytes) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to read avatar bytes: {:?}", e);
            return;
        }
    };
    info!("Received avatar file: {} bytes (TODO: wire into PhotonApp)", bytes.len());
}

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

// FCM Push Notification Support ============================================================================
use std::sync::atomic::{AtomicBool, Ordering};

/// Flag set by FCM service when peer update received - triggers FGTW refresh
static FCM_PEER_UPDATE_PENDING: AtomicBool = AtomicBool::new(false);

/// Check and clear the FCM peer update flag
#[cfg(target_os = "android")]
pub fn check_fcm_peer_update() -> bool {
    FCM_PEER_UPDATE_PENDING.swap(false, Ordering::SeqCst)
}

/// Called from FirebaseMessagingService when peer_update FCM message received This is called from a background thread, so we just set a flag
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonMessagingService_nativePeerUpdateReceived(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) {
    info!("FCM peer update received - flagging for refresh");
    FCM_PEER_UPDATE_PENDING.store(true, Ordering::SeqCst);
}

// ============================================================================

// PhotonConnectionService - Background Network Stack ============================================================================
/// Derive device keypair from fingerprint bytes using BLAKE3
#[cfg(target_os = "android")]
fn derive_device_keypair(fingerprint: &[u8]) -> Keypair {
    use ed25519_dalek::SigningKey;
    let hash = blake3::hash(fingerprint);
    let seed: [u8; 32] = *hash.as_bytes();
    let secret = SigningKey::from_bytes(&seed);
    let public = secret.verifying_key();
    Keypair { secret, public }
}

/// Network context owned by the foreground service. Persists across Activity lifecycle changes; holds the device keypair and peer store. The Activity creates its own HandleQuery on demand via the shared transport.
#[cfg(target_os = "android")]
pub struct NetworkContext {
    pub keypair: Keypair,
    pub peer_store: Arc<Mutex<PeerStore>>,
    /// Primary ring directory — Activity passes `context.filesDir.absolutePath` (app-private internal storage, `/data/user/0/<pkg>/files`).
    pub data_dir: String,
    /// Shadow ring directory — Activity passes `context.getExternalFilesDir(null)?.absolutePath` (app-private external, `/storage/emulated/0/Android/data/<pkg>/files`). Empty string if external storage wasn't available; storage layer falls back to a shadow-suffix file inside `data_dir` in that case.
    pub shadow_dir: String,
}

#[cfg(target_os = "android")]
impl NetworkContext {
    pub fn new(fingerprint: &[u8], data_dir: &str, shadow_dir: &str) -> Self {
        // Set global Android data directory for avatar storage
        crate::avatar::set_android_data_dir(data_dir.to_string());
        // Hand the storage layer both ring dirs so the dual-ring vault can place primary on internal and shadow on external — see [storage::flat::set_android_vault_dirs].
        crate::storage::flat::set_android_vault_dirs(data_dir.to_string(), shadow_dir.to_string());

        let keypair = derive_device_keypair(fingerprint);

        info!(
            "NetworkContext: Device pubkey: {}",
            hex::encode(keypair.public.as_bytes())
        );

        let peer_store = Arc::new(Mutex::new(PeerStore::new()));

        Self {
            keypair,
            peer_store,
            data_dir: data_dir.to_string(),
            shadow_dir: shadow_dir.to_string(),
        }
    }

    /// Poll for network events (called periodically from service background thread)
    pub fn poll(&self) {
        // Transport handles incoming UDP internally This hook is for any periodic maintenance
    }
}

#[cfg(target_os = "android")]
fn get_network_context(ptr: jlong) -> Option<&'static mut NetworkContext> {
    if ptr == 0 {
        error!("Null NetworkContext pointer received");
        return None;
    }
    unsafe { Some(&mut *(ptr as *mut NetworkContext)) }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonConnectionService_nativeNetworkInit(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    fingerprint: JByteArray<'_>,
    data_dir: JString<'_>,
    shadow_dir: JString<'_>,
) -> jlong {
    info!("PhotonConnectionService: Initializing network stack");

    let fingerprint_bytes = match env.convert_byte_array(&fingerprint) {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to read fingerprint bytes: {:?}", e);
            return 0;
        }
    };

    let data_dir_str: String = match env.get_string(&data_dir) {
        Ok(s) => s.into(),
        Err(e) => {
            error!("Failed to read data_dir: {:?}", e);
            return 0;
        }
    };

    let shadow_dir_str: String = match env.get_string(&shadow_dir) {
        Ok(s) => s.into(),
        Err(e) => {
            error!("Failed to read shadow_dir: {:?}", e);
            return 0;
        }
    };

    info!(
        "NetworkContext: fingerprint {} bytes, data_dir: {}, shadow_dir: {}",
        fingerprint_bytes.len(),
        data_dir_str,
        if shadow_dir_str.is_empty() { "<none>" } else { &shadow_dir_str },
    );

    let context = Box::new(NetworkContext::new(
        &fingerprint_bytes,
        &data_dir_str,
        &shadow_dir_str,
    ));
    Box::into_raw(context) as jlong
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonConnectionService_nativeNetworkDestroy(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    network_ptr: jlong,
) {
    if network_ptr != 0 {
        info!("Destroying NetworkContext");
        unsafe {
            let _ = Box::from_raw(network_ptr as *mut NetworkContext);
        }
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonConnectionService_nativeNetworkPoll(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    network_ptr: jlong,
) {
    let Some(context) = get_network_context(network_ptr) else {
        return;
    };
    context.poll();
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonConnectionService_nativeGetDevicePubkey<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    network_ptr: jlong,
) -> JString<'local> {
    let empty = || env.new_string("").unwrap();
    let Some(context) = get_network_context(network_ptr) else {
        return empty();
    };
    let hex = hex::encode(context.keypair.public.as_bytes());
    env.new_string(&hex).unwrap_or_else(|_| empty())
}
