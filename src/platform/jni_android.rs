//! JNI bindings for Android.
//!
//! Two consumer surfaces hit these symbols:
//! - `PhotonActivity` (the foreground UI) → `Java_com_photon_messenger_PhotonActivity_native*`.
//!   These are thin shims into `fluor::host::android::AndroidShell<PhotonApp>` — the shell owns the surface + render pipeline + event translation, photon owns only the FluorApp impl and the app-specific bits (avatar picker, FCM peer-update flag).
//! - `PhotonConnectionService` (the background network stack) → `Java_com_photon_messenger_PhotonConnectionService_nativeNetwork*` + FCM peer-update notification. These survive the UI Activity's lifecycle so the persistent peer store + device keypair don't churn on each rotation / background trip.

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

/// The message-notification sink: the JavaVM + a global ref of the PhotonConnectionService, registered at `nativeNetworkInit`. Lets any Rust thread (the status RX worker, chiefly) call up into Kotlin's `postMessageNotification()` — the service outlives the Activity, so this works while the app is backgrounded.
#[cfg(target_os = "android")]
static MESSAGE_NOTIFIER: std::sync::OnceLock<(jni::JavaVM, jni::objects::GlobalRef)> =
    std::sync::OnceLock::new();

/// The identity of the last message we fired a notification for, so a retransmit of the SAME logical message doesn't re-ding. A dozing/off-LAN peer retransmits the same frame many times (observed: one message → 5+ retransmits, and its ACK echoed 10× in a single millisecond), and `notify()` re-alerts the sound on every call even under a fixed notification id — so without this gate one message becomes a burst of dings. Keyed on the message's `prev_msg_hp` (unique per position in the chain).
#[cfg(target_os = "android")]
static LAST_NOTIFIED_MSG: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);

/// Fire the Android "new message" notification for the message identified by `msg_hp` (its `prev_msg_hp` chain position), sounding + buzzing the SENDER's per-contact chirp. Deduplicated: a repeat of the same `msg_hp` (a retransmit) is a no-op, so one logical message dings once.
///
/// The sound/haptic are the sender's deterministic chirp (`chirp::Chirp::from_hash(blake3(sender_pubkey))`) rendered here to a WAV + the matching amplitude-envelope haptic, and handed to Kotlin, which plays them itself (silent channel + MediaPlayer + VibrationEffect) — the "app plays it after wake" path, so the OS default tone never fires and the sound is per-contact even from deep Doze. Sender identity stays in-process: only the rendered audio/haptic cross to Kotlin, never the pubkey or plaintext, and the notification text stays a generic "New message" (handle off the lock screen).
///
/// Kotlin decides visibility/foreground suppression. No-op if the service never registered. Callable from any thread — attaches to the JVM as needed.
#[cfg(target_os = "android")]
pub fn notify_new_message(msg_hp: &[u8; 32], sender_pubkey: &[u8]) {
    // Dedup: skip if this is the same message we most recently notified for (a retransmit).
    {
        let mut last = LAST_NOTIFIED_MSG.lock().unwrap();
        if last.as_ref() == Some(msg_hp) {
            return;
        }
        *last = Some(*msg_hp);
    }
    let Some((vm, svc)) = MESSAGE_NOTIFIER.get() else {
        return;
    };

    // Derive the sender's chirp → WAV bytes + haptic (timings, amplitudes). blake3(pubkey) gives a clean 32-byte seed; same sender → same chirp every time. 60 Hz bins the envelope into ~16.7ms steps.
    let seed: [u8; 32] = *blake3::hash(sender_pubkey).as_bytes();
    let chirp = chirp::Chirp::from_hash(seed);
    let wav = chirp.to_wav();
    let (timings, amplitudes) = chirp.haptic_waveform(60);

    match vm.attach_current_thread() {
        Ok(mut env) => {
            // Marshal WAV (byte[]), timings (long[] — createWaveform wants long[]), amplitudes (int[] — createWaveform wants int[]).
            let wav_arr = match env.byte_array_from_slice(&wav) {
                Ok(a) => a,
                Err(e) => {
                    error!("notify_new_message: wav array alloc failed: {:?}", e);
                    return;
                }
            };
            let timings_i64: Vec<i64> = timings.iter().map(|&t| t as i64).collect();
            let timings_arr = match env.new_long_array(timings_i64.len() as i32) {
                Ok(a) => {
                    let _ = env.set_long_array_region(&a, 0, &timings_i64);
                    a
                }
                Err(e) => {
                    error!("notify_new_message: timings array alloc failed: {:?}", e);
                    return;
                }
            };
            let amps_i32: Vec<i32> = amplitudes.iter().map(|&a| a as i32).collect();
            let amps_arr = match env.new_int_array(amps_i32.len() as i32) {
                Ok(a) => {
                    let _ = env.set_int_array_region(&a, 0, &amps_i32);
                    a
                }
                Err(e) => {
                    error!("notify_new_message: amplitudes array alloc failed: {:?}", e);
                    return;
                }
            };

            if env
                .call_method(
                    svc.as_obj(),
                    "postMessageNotification",
                    "([B[J[I)V",
                    &[
                        (&wav_arr).into(),
                        (&timings_arr).into(),
                        (&amps_arr).into(),
                    ],
                )
                .is_err()
            {
                let _ = env.exception_clear();
                error!("notify_new_message: postMessageNotification call failed");
            }
        }
        Err(e) => error!("notify_new_message: JVM attach failed: {:?}", e),
    }
}

/// Poke the foreground service to run a headless protocol tick. Called from the status RX worker (`send_status_update`) whenever ANY inbound `StatusUpdate` lands, so a CLUTCH offer/KEM/complete or a chat/ACK advances the ceremony + chain even while the Activity is backgrounded and its Choreographer (and thus `tick`) has stopped. Reuses the `MESSAGE_NOTIFIER` service global-ref: calls Kotlin `requestServiceTick()`, which grabs a brief wakelock and calls `nativeServiceTick(activityPtr)`. The wakelock lives on the Kotlin side because it needs the service `Context`/`PowerManager`. No-op if the service never registered or the Activity context ptr isn't set (Kotlin guards that). Callable from any thread — attaches to the JVM as needed. See docs/background-tick.md.
#[cfg(target_os = "android")]
pub fn request_service_tick() {
    let Some((vm, svc)) = MESSAGE_NOTIFIER.get() else {
        return;
    };
    match vm.attach_current_thread() {
        Ok(mut env) => {
            if env
                .call_method(svc.as_obj(), "requestServiceTick", "()V", &[])
                .is_err()
            {
                let _ = env.exception_clear();
                error!("request_service_tick: requestServiceTick call failed");
            }
        }
        Err(e) => error!("request_service_tick: JVM attach failed: {:?}", e),
    }
}

// PhotonActivity context — wraps fluor::AndroidShell<PhotonApp> ============================================================================
/// Activity-side context. Holds the fluor shell that owns the FluorApp + surface + pipeline. Lifetime: created on Activity surface-creation (`nativeInitWithNetwork`), destroyed on Activity teardown (`nativeDestroy`).
#[cfg(target_os = "android")]
pub struct PhotonContext {
    pub shell: AndroidShell<PhotonApp>,
    /// Re-entry guard between the Activity draw thread (`nativeDraw` → `tick`) and the foreground-service thread (`nativeServiceTick` → `advance_protocol`). Both mutate the one `PhotonApp` thru the `&'static mut` handed out by `get_context`, so they must never run concurrently. They normally can't — the frame callback is removed on `onPause`, exactly when the service tick takes over — but the `onResume` re-arm can momentarily overlap. This flag serialises them: whoever holds it runs, the other skips (a skipped background tick is harmless — the next foreground `tick` drains the same channels anyway). See docs/background-tick.md.
    pub ticking: std::sync::atomic::AtomicBool,
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
            ticking: std::sync::atomic::AtomicBool::new(false),
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
    // Hold the tick guard across the whole draw (which runs `tick` → `advance_protocol` internally): a background `nativeServiceTick` racing us on `onResume` will see it busy and skip. The draw is the foreground owner and always wins; the deferred background tick is a harmless no-op because this very draw drains the same channels. `Acquire`/`Release` order the flag against the mutations.
    use std::sync::atomic::Ordering;
    ctx.ticking.store(true, Ordering::Relaxed);
    ctx.shell.draw(&window);
    ctx.ticking.store(false, Ordering::Release);
}

/// Headless protocol advance, called from the foreground **service** thread (`PhotonConnectionService`) when inbound traffic arrives while the Activity is backgrounded — the Choreographer has stopped calling `nativeDraw`, so `tick` isn't running and CLUTCH/chat would otherwise stall until the screen comes on. Runs the surface-free `PhotonApp::advance_protocol` (drain channels, advance the ceremony + chain, retransmit) with NO drawing. Skips if a draw is concurrently in progress (the `onResume` overlap) — that draw covers the same work. Reuses the Activity `PhotonContext` ptr, which stays valid while the app is merely paused (destroyed only at `onDestroy`; a 0/stale ptr here is a no-op).
/// See docs/background-tick.md.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonConnectionService_nativeServiceTick(
    _env: JNIEnv<'_>,
    _class: JObject<'_>,
    context_ptr: jlong,
) {
    use std::sync::atomic::Ordering;
    let Some(ctx) = get_context(context_ptr) else {
        return;
    };
    // CAS false→true: acquire the guard only if no draw (or another service tick) holds it. On failure we skip — the concurrent draw advances the same state, so a dropped background tick loses nothing.
    if ctx
        .ticking
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let now = std::time::Instant::now();
    ctx.shell.app().advance_protocol(now);
    ctx.ticking.store(false, Ordering::Release);
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

/// Returns: 1=show keyboard, -1=hide keyboard, 0=no change. AndroidShell::on_touch reads PhotonApp::wants_keyboard after dispatching the touch thru the widget tree; the JNI shim just forwards the int.
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

/// Push the device's display colour-space data from Kotlin into fluor's `theme` globals. `rgb_to_xyz` is a 9-float row-major 3x3 matrix mapping the display's RGB into CIE XYZ D50 (queried Kotlin-side from `display.preferredWideGamutColorSpace.transform`). `primaries` is 6 floats `[Rx, Ry, Gx, Gy, Bx, By]` from the same ColorSpace. Stored once at Activity init; consumers (chromatic_wave, future colour-managed painters) read via `fluor::theme::display_rgb_to_xyz()` and `display_primaries()`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeSetDisplayColorSpace(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    rgb_to_xyz: jni::objects::JFloatArray<'_>,
    primaries: jni::objects::JFloatArray<'_>,
) {
    let mut m = [0f32; 9];
    let mut p = [0f32; 6];
    if let Err(e) = env.get_float_array_region(&rgb_to_xyz, 0, &mut m) {
        error!(
            "nativeSetDisplayColorSpace: rgb_to_xyz read failed: {:?}",
            e
        );
        return;
    }
    if let Err(e) = env.get_float_array_region(&primaries, 0, &mut p) {
        error!("nativeSetDisplayColorSpace: primaries read failed: {:?}", e);
        return;
    }
    info!(
        "Display ColourSpace: rgb→XYZ = [{:.4} {:.4} {:.4} / {:.4} {:.4} {:.4} / {:.4} {:.4} {:.4}]  primaries Rxy=({:.4},{:.4}) Gxy=({:.4},{:.4}) Bxy=({:.4},{:.4})",
        m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8],
        p[0], p[1], p[2], p[3], p[4], p[5]
    );
    fluor::theme::set_display_color_space(m, p);
}

/// Per-frame poll for the soft-keyboard show/hide signal. Returns `1` / `-1` / `0` like `nativeOnTouch`. Called from `PhotonActivity.doFrame` so app-driven focus changes (e.g. `change_focus(None)` from `submit_handle` while attesting) reach the Activity without waiting for the next user touch. Cheap — boils down to `app.wants_keyboard()` which is a take-on-change one-shot.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativePollKeyboard(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
) -> jint {
    let Some(ctx) = get_context(context_ptr) else {
        return 0;
    };
    ctx.shell.poll_keyboard() as jint
}

/// Per-frame poll: `1` when the app just cleared its compose box (a message was sent) and the Activity should `InputMethodManager.restartInput` to drop the IME's stale composing buffer, else `0`. One-shot.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativePollInputReset(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
) -> jint {
    let Some(ctx) = get_context(context_ptr) else {
        return 0;
    };
    ctx.shell.poll_input_reset() as jint
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

/// Avatar from image picker. NOT in AndroidShell — photon-specific (decodes via the existing avatar pipeline). Funnels raw file bytes (JPEG/PNG/WebP — Android side intentionally does NOT decode thru `BitmapFactory` because that destroys ICC profile data) thru `PhotonApp::set_avatar_from_file`, which encodes to VSF, saves to the encrypted handle-keyed store, reloads, colour-converts to BT.2020 γ=2.0 for the surface buffer, and (when a handle_proof is available) uploads to FGTW.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeSetAvatarFromFile(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
    file_bytes: JByteArray<'_>,
) {
    let Some(ctx) = get_context(context_ptr) else {
        return;
    };
    let bytes = match env.convert_byte_array(&file_bytes) {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to read avatar bytes: {:?}", e);
            return;
        }
    };
    ctx.shell.app().set_avatar_from_file(bytes);
}

/// Per-frame poll for the sticky session broadcast signal. Returns `1` after a successful attest (Kotlin should call `service.sendSessionBroadcast()`), `-1` after a vault nuke (Kotlin should call `service.clearSessionBroadcast()`), `0` otherwise. One-shot.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativePollSessionBroadcast(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
) -> jint {
    let Some(ctx) = get_context(context_ptr) else {
        return 0;
    };
    ctx.shell.app().take_broadcast_signal() as jint
}

/// Per-frame poll for the avatar image-picker request. Returns `1` when the user has tapped the avatar circle since the last poll, `0` otherwise. Kotlin's `doFrame` hook calls this alongside `nativePollKeyboard` and launches `ACTION_GET_CONTENT` on `1`. One-shot semantics: `PhotonApp::take_picker_request` clears the flag so consecutive polls without further taps yield `0`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativePollAvatarPicker(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    context_ptr: jlong,
) -> jint {
    let Some(ctx) = get_context(context_ptr) else {
        return 0;
    };
    if ctx.shell.app().take_picker_request() {
        1
    } else {
        0
    }
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
        crate::storage::set_android_vault_dirs(data_dir.to_string(), shadow_dir.to_string());
        // Route the structured VSF log to the EXTERNAL files dir (shadow): a release dev APK isn't `run-as`-able, so an internal-storage log can't be pulled — but the external dir IS adb-readable.
        // Empty shadow_dir (no external storage) falls back to internal in the sink.
        #[cfg(feature = "logging")]
        crate::set_android_log_dir(shadow_dir.to_string());

        // Wire tohu's boot-locked session capsule to the app dirs — WITHOUT this, session_capsule_paths() is None, set_session falls thru to the desktop XDG tmpfs path (absent on Android) and FAILS, so the session never persists: attest succeeds but self.session stays None (avatar picker "not attested", broadcast "no session stored") and every restart lands back on the attest screen.
        let primary = std::path::Path::new(data_dir).join("session");
        let shadow = if shadow_dir.is_empty() {
            None
        } else {
            Some(std::path::Path::new(shadow_dir).join("session"))
        };
        tohu::set_session_dir(&primary, shadow.as_deref());

        // tohu reads Settings.Secure.ANDROID_ID itself (via the JavaVM handed to it in JNI_OnLoad). Fall back to the Java-pushed `fingerprint` if tohu's fetch errors, so a wrong JNI path logs loudly instead of bricking the app. NOTE: switching the oracle to pure ANDROID_ID changes device_secret vs the old Java-pushed value — existing Android vaults must be cleared.
        let oracle = match tohu::device::machine_fingerprint() {
            Ok(id) => {
                info!(
                    "NetworkContext: device oracle via tohu (ANDROID_ID), {} bytes",
                    id.len()
                );
                id
            }
            Err(e) => {
                error!(
                    "NetworkContext: tohu device oracle failed ({e}); falling back to Java-pushed fingerprint"
                );
                fingerprint.to_vec()
            }
        };
        let keypair = derive_device_keypair(&oracle);

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
    service: JObject<'_>,
    fingerprint: JByteArray<'_>,
    data_dir: JString<'_>,
    shadow_dir: JString<'_>,
) -> jlong {
    info!("PhotonConnectionService: Initializing network stack");

    // Register the service as the message-notification sink. nativeNetworkInit is an INSTANCE method on the Kotlin side, so the second JNI parameter is the service `this` — a global ref of it (+ the JavaVM) lets the RX thread post "new message" notifications up thru Kotlin regardless of Activity lifecycle (the Choreographer poll bridge stops when the app backgrounds, which is exactly when notifications matter).
    match (env.get_java_vm(), env.new_global_ref(&service)) {
        (Ok(vm), Ok(svc)) => {
            let _ = MESSAGE_NOTIFIER.set((vm, svc));
        }
        (vm, svc) => error!(
            "nativeNetworkInit: notifier registration failed (vm ok: {}, ref ok: {})",
            vm.is_ok(),
            svc.is_ok()
        ),
    }

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
        if shadow_dir_str.is_empty() {
            "<none>"
        } else {
            &shadow_dir_str
        },
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
pub extern "C" fn Java_com_photon_messenger_PhotonConnectionService_nativeGetDevicePubkey<
    'local,
>(
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

/// Restore a session from a VSF capsule read from the sticky broadcast. Called on first launch after reinstall, before the app UI initialises, so `query_resume` can skip re-attest.
/// Returns `1` if the session was restored, `0` if the capsule was invalid or empty.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonActivity_nativeRestoreSessionFromVsf(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    vsf_bytes: JByteArray<'_>,
) -> jint {
    let bytes = match env.convert_byte_array(&vsf_bytes) {
        Ok(b) => b,
        Err(e) => {
            error!("nativeRestoreSessionFromVsf: read bytes failed: {:?}", e);
            return 0;
        }
    };
    if bytes.is_empty() {
        return 0;
    }
    match unpack_session_vsf(&bytes) {
        Some(session) => match tohu::set_session(&session) {
            Ok(()) => {
                info!("Session restored from sticky broadcast");
                1
            }
            Err(e) => {
                error!("nativeRestoreSessionFromVsf: set_session failed: {:?}", e);
                0
            }
        },
        None => {
            error!("nativeRestoreSessionFromVsf: unpack failed");
            0
        }
    }
}

// ============================================================================
// Session broadcast — sticky broadcast carrying the VSF-sealed session capsule.
// Survives app uninstall (OS holds the sticky); cleared on logout.
// Permission: com.photon.SESSION_READ (signature-level, declared in manifest).
// ============================================================================

/// Seal a SessionIdentity for the sticky broadcast: the boot-locked `Shared` capsule — spaghettify(boot_id ‖ device_secret) AEAD + device_secret MAC (docs/android-session-persistence.md).
/// The OLD packing here was a PLAINTEXT VSF (raw hI/hV/hP, hb integrity only) — a stale sticky from a previous boot would have restored raw roots straight across a reboot, violating the power-rail boundary the wairua exists to enforce. The sealed capsule fails AEAD on any other boot, so the "None → attest" path fires exactly as designed.
#[cfg(target_os = "android")]
fn pack_session_vsf(s: &tohu::SessionIdentity) -> Option<Vec<u8>> {
    tohu::seal_session(s, tohu::SealMode::Shared)
}

/// Open a broadcast capsule produced by `pack_session_vsf`. `None` on MAC mismatch, AEAD failure (wrong boot = reboot), tamper, or truncation.
#[cfg(target_os = "android")]
pub fn unpack_session_vsf(bytes: &[u8]) -> Option<tohu::SessionIdentity> {
    tohu::open_session(bytes, tohu::SealMode::Shared)
}

/// Send (or replace) the sticky session broadcast. Reads the current session from `tohu::session()` — seeds never leave Rust. Called from Kotlin after attest succeeds.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonConnectionService_nativeSendSessionBroadcast(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    context: JObject<'_>,
) {
    let session = match tohu::session() {
        Some(s) => s,
        None => {
            error!("nativeSendSessionBroadcast: no session stored");
            return;
        }
    };
    let bytes = match pack_session_vsf(&session) {
        Some(b) => b,
        None => {
            error!("nativeSendSessionBroadcast: seal failed (no boot secret or device_secret)");
            return;
        }
    };
    let result = (|| -> Result<(), jni::errors::Error> {
        let intent_class = env.find_class("android/content/Intent")?;
        let action = env.new_string("com.photon.SESSION")?;
        let intent = env.new_object(&intent_class, "(Ljava/lang/String;)V", &[(&action).into()])?;
        let extra_key = env.new_string("vsf")?;
        let arr = env.byte_array_from_slice(&bytes)?;
        env.call_method(
            &intent,
            "putExtra",
            "(Ljava/lang/String;[B)Landroid/content/Intent;",
            &[(&extra_key).into(), (&arr).into()],
        )?;
        env.call_method(
            &context,
            "sendStickyBroadcast",
            "(Landroid/content/Intent;)V",
            &[(&intent).into()],
        )?;
        Ok(())
    })();
    if let Err(e) = result {
        error!("nativeSendSessionBroadcast failed: {:?}", e);
    } else {
        info!("Session broadcast sent ({} bytes)", bytes.len());
    }
}

/// Clear the sticky session broadcast. Called from Kotlin on logout.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_photon_messenger_PhotonConnectionService_nativeClearSessionBroadcast(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    context: JObject<'_>,
) {
    let result = (|| -> Result<(), jni::errors::Error> {
        let intent_class = env.find_class("android/content/Intent")?;
        let action = env.new_string("com.photon.SESSION")?;
        let intent = env.new_object(&intent_class, "(Ljava/lang/String;)V", &[(&action).into()])?;
        env.call_method(
            &context,
            "removeStickyBroadcast",
            "(Landroid/content/Intent;)V",
            &[(&intent).into()],
        )?;
        Ok(())
    })();
    if let Err(e) = result {
        error!("nativeClearSessionBroadcast failed: {:?}", e);
    } else {
        info!("Session broadcast cleared");
    }
}
