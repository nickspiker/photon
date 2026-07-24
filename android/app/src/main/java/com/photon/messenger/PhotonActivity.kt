package com.photon.messenger

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.content.res.Resources
import android.database.ContentObserver
import android.graphics.PixelFormat
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.provider.Settings
import android.view.Choreographer
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.google.firebase.messaging.FirebaseMessaging

class PhotonActivity : AppCompatActivity(), SurfaceHolder.Callback, Choreographer.FrameCallback {

    companion object {
        init {
            System.loadLibrary("photon_messenger")
        }
        // _v2: the channel went SILENT (no channel sound/vibration — the app plays the per-contact
        // chirp + haptic itself). A channel's sound/vibration are immutable after first creation, so a
        // new id is the only way the silence takes effect on devices that already had the old channel.
        const val CHANNEL_ID = "photon_messages_v2"
        const val CHANNEL_NAME = "Messages"
        private const val TAG = "PhotonActivity"

        /** True while the Activity is between onResume and onPause — the service's message notification suppresses itself when the user is already looking at the app. @Volatile: read from the Rust RX thread's upcall. */
        @Volatile
        var inForeground = false
    }

    // Native UI context pointer (rendering, touch, etc.)
    private var nativePtr: Long = 0

    // Service binding for network stack
    private var connectionService: PhotonConnectionService? = null
    private var serviceBound = false

    // Surface for rendering (custom class with InputConnection)
    private lateinit var surfaceView: PhotonSurfaceView
    private var surfaceReady = false

    // Device fingerprint result
    private var fingerprintResult: DeviceFingerprint.FingerprintResult? = null

    // Track full screen height (before keyboard)
    private var fullHeight: Int = 0
    private var keyboardVisible = false

    // Scale gesture detector for pinch-to-zoom
    private lateinit var scaleGestureDetector: ScaleGestureDetector

    // Gravity sensor → setRequestedOrientation. We let the OS own the rotation (so the IME and any system overlays render in the correct orientation), but drive it ourselves from the gravity vector with a 7.0 m/s² deadband — same instant feel as lumis without the OrientationEventListener debounce (~500ms). Paired with ROTATION_ANIMATION_JUMPCUT, the orientation change shows as a hard cut with no rotation animation. Tracking `requestedOrientation` ourselves so we only call `setRequestedOrientation` on actual change (the setter triggers a config delivery either way).
    private lateinit var sensorManager: SensorManager
    private var gravitySensor: Sensor? = null
    private var gravityX: Float = 0f
    private var gravityY: Float = 9.8f
    private var gravityZ: Float = 0f
    private var currentOrientation: Int = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT

    // HONOR THE USER'S ROTATION SETTINGS while keeping OUR rotation FEEL (instant, 7 m/s² deadband, jumpcut — never the OS listener's ~500ms lag or its flat-table jitter). We drive WHEN to rotate from gravity; the OS decides only WHICH angles are allowed:
    //  • autoRotateEnabled — Settings.System.ACCELEROMETER_ROTATION (live via a ContentObserver). Off = user locked rotation → we stop sensor-driving and hand the LOCKED angle to SCREEN_ORIENTATION_USER (Android holds their chosen orientation; we don't compute it).
    //  • allowReversePortrait — the framework's config_allowAllRotations bool, read once by resource-identifier (NOT hidden-API reflection; resource lookups aren't blocklisted). Most phones disallow 180°; when they do, our gravity snap skips the reverse-portrait branch instead of forcing it.
    private var autoRotateEnabled: Boolean = true
    private val allowReversePortrait: Boolean by lazy {
        val id = Resources.getSystem().getIdentifier("config_allowAllRotations", "bool", "android")
        id != 0 && Resources.getSystem().getBoolean(id)
    }
    private val rotationSettingObserver = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean) {
            autoRotateEnabled = readAutoRotate()
            updateOrientationFromGravity() // apply the toggle immediately, don't wait for the next sensor tick
        }
    }
    private fun readAutoRotate(): Boolean =
        Settings.System.getInt(contentResolver, Settings.System.ACCELEROMETER_ROTATION, 1) == 1

    private val gravityListener = object : SensorEventListener {
        override fun onSensorChanged(event: SensorEvent) {
            if (event.sensor.type == Sensor.TYPE_GRAVITY) {
                gravityX = event.values[0]
                gravityY = event.values[1]
                gravityZ = event.values[2]
                updateOrientationFromGravity()
            }
        }
        override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
    }

    /// Snap raw gravity to one of the four screen orientations with a 7.0 m/s² deadband (≈71% of g, well past the 45° diagonal). Below threshold on both horizontal axes (phone near-flat) the previous orientation is retained — this is what keeps a phone flat on a table from flapping (the OS listener has no such deadband, which is exactly why it jitters). Gated + filtered by the user's rotation settings: their lock and their allowed angles, our timing.
    private fun updateOrientationFromGravity() {
        if (!autoRotateEnabled) {
            // User locked rotation — honor it. SCREEN_ORIENTATION_USER holds their chosen angle; we don't sensor-drive and we don't compute the angle (Android maps USER_ROTATION for us).
            if (currentOrientation != ActivityInfo.SCREEN_ORIENTATION_USER) {
                currentOrientation = ActivityInfo.SCREEN_ORIENTATION_USER
                requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_USER
            }
            return
        }
        val target = when {
            gravityY > 7.0f -> ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
            gravityX < -7.0f -> ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE
            // Reverse-portrait only if the device permits it (config_allowAllRotations); otherwise hold — don't force an angle the user's rules disallow.
            gravityY < -7.0f -> if (allowReversePortrait) ActivityInfo.SCREEN_ORIENTATION_REVERSE_PORTRAIT else currentOrientation
            gravityX > 7.0f -> ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
            else -> currentOrientation
        }
        if (target != currentOrientation) {
            currentOrientation = target
            requestedOrientation = target
        }
    }

    // Service connection callbacks
    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
            val localBinder = binder as PhotonConnectionService.LocalBinder
            connectionService = localBinder.getService()
            serviceBound = true
            PhotonLog.d(TAG, "Bound to PhotonConnectionService")
            // Try to initialize UI now that service is ready
            initializeNativeIfReady()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            connectionService = null
            serviceBound = false
            PhotonLog.d(TAG, "Disconnected from PhotonConnectionService")
        }
    }

    // Native methods for UI (network is in service)
    private external fun nativeInitWithNetwork(width: Int, height: Int, networkPtr: Long, isSamsung: Boolean): Long
    private external fun nativeDraw(contextPtr: Long, surface: android.view.Surface)
    private external fun nativeResize(contextPtr: Long, width: Int, height: Int)
    private external fun nativeOnTouch(contextPtr: Long, action: Int, x: Float, y: Float): Int  // Returns: 1=show keyboard, -1=hide keyboard, 2=open image picker, 0=no change
    private external fun nativeOnTextInput(contextPtr: Long, text: String)  // Text from soft keyboard
    private external fun nativeOnKeyEvent(contextPtr: Long, keyCode: Int): Boolean  // Special keys (backspace, enter)
    private external fun nativeOnBackPressed(contextPtr: Long): Boolean  // Back button - returns true if handled
    private external fun nativeOnScale(contextPtr: Long, scaleFactor: Float)  // Pinch-to-zoom scale factor
    private external fun nativePollKeyboard(contextPtr: Long): Int  // Per-frame poll for show/hide soft IME — 1=show, -1=hide, 0=no change
    private external fun nativePollInputReset(contextPtr: Long): Int  // Per-frame poll: 1=restartInput (clear the IME's stale composing buffer after a send), 0=no change
    private external fun nativePollAvatarPicker(contextPtr: Long): Int  // Per-frame poll for the avatar image-picker request — 1=launch ACTION_GET_CONTENT, 0=no change
    private external fun nativePollSessionBroadcast(contextPtr: Long): Int  // 1=send sticky broadcast, -1=clear, 0=no change
    private external fun nativePollApkInstall(contextPtr: Long): String?  // Per-frame poll: staged self-update APK path (one-shot) — fire the system installer with it
    private external fun nativePollClipboardCopy(contextPtr: Long): String?  // Per-frame poll: text to place on the OS clipboard (one-shot) — copy-words / copy-name affordances
    private external fun nativeSetDisplayColorSpace(rgbToXyz: FloatArray, primaries: FloatArray)  // Display panel's RGB→XYZ_D50 (9 floats) + chromaticity primaries [Rx,Ry,Gx,Gy,Bx,By] (6 floats)
    private external fun nativeSetAvatarFromFile(contextPtr: Long, fileBytes: ByteArray)  // Raw image file bytes (preserves ICC profile)
    private external fun nativeRestoreSessionFromVsf(vsfBytes: ByteArray): Int  // Restore session from sticky broadcast VSF capsule — call before nativeInitWithNetwork; returns 1 on success
    private external fun nativeDestroy(contextPtr: Long)

    // Notification permission request (Android 13+)
    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { isGranted ->
        if (isGranted) {
            createNotificationChannel()
        }
    }

    // BLE beacon permissions (Android 12+): requested lazily by PhotonBeacon when a start call
    // finds them missing; on grant the pending advertise/scan re-runs so the pairing screen
    // doesn't need re-entering.
    private val blePermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { grants ->
        if (grants.values.all { it }) {
            PhotonBeacon.onPermissionsGranted()
        } else {
            PhotonLog.w("Beacon", "BLE permissions denied")
        }
    }

    fun requestBlePermissions() {
        if (Build.VERSION.SDK_INT >= 31) {
            blePermissionLauncher.launch(arrayOf(
                Manifest.permission.BLUETOOTH_SCAN,
                Manifest.permission.BLUETOOTH_ADVERTISE
            ))
        }
    }

    // Image picker for avatar selection — passes RAW FILE BYTES to Rust. We do NOT decode in Android because BitmapFactory destroys ICC profiles and mangles colors; Rust handles proper color management via XYZ.
    private val imagePickerLauncher = registerForActivityResult(
        ActivityResultContracts.GetContent()
    ) { uri ->
        uri?.let {
            try {
                contentResolver.openInputStream(it)?.use { stream ->
                    val fileBytes = stream.readBytes()
                    if (nativePtr != 0L && fileBytes.isNotEmpty()) {
                        nativeSetAvatarFromFile(nativePtr, fileBytes)
                    }
                }
            } catch (e: Exception) {
                // Silently fail - Rust will log if needed
            }
        }
    }

    private fun openImagePicker() {
        imagePickerLauncher.launch("image/*")
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Pairing v2 beacon bridge: cache contexts + register the JNI upcall path before any
        // screen can ask the radio for anything (docs/pairing-v2.md).
        PhotonBeacon.init(this)
        PhotonNfc.init(this)

        // Create custom SurfaceView with InputConnection for IME text input
        val container = FrameLayout(this)
        surfaceView = PhotonSurfaceView(this) { text ->
            // Callback for committed text from IME (voice, swipe, autocomplete)
            if (nativePtr != 0L) {
                if (text == "\b") {
                    // Backspace from deleteSurroundingText
                    nativeOnKeyEvent(nativePtr, KeyEvent.KEYCODE_DEL)
                } else {
                    nativeOnTextInput(nativePtr, text)
                }
            }
        }
        container.addView(surfaceView, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.MATCH_PARENT,
            FrameLayout.LayoutParams.MATCH_PARENT
        ))
        setContentView(container)

        // Use RGBA_8888 for efficient u32 pixel copies from Rust
        surfaceView.holder.setFormat(PixelFormat.RGBA_8888)
        surfaceView.holder.addCallback(this)

        // ROTATION_ANIMATION_JUMPCUT: when setRequestedOrientation flips the screen, do a hard cut with no rotation animation. This is the entire point of driving rotation ourselves — without it, every orientation change pays for the OS's default rotate animation (~300-500ms).
        //
        // colorMode = WIDE_COLOR_GAMUT: opt the Surface into the device's wide gamut (Display P3 on the Pixel 8 Pro) instead of the sRGB default. preferMinimalPostProcessing (API 30+) asks the compositor to skip vendor colour adjustments (saturation boost, "Vivid" mode, etc.). Together with `ANativeWindow_setBuffersDataSpace(ADATASPACE_DISPLAY_P3)` on the Rust side, this gives us a display-native pipeline: bytes we write land on the panel without an sRGB-clamp. Photon does its own colour management later in the paint pipeline (theme constants + chromatic wave get transformed before they're written to the framebuffer), so the OS-side clamp would only fight that work.
        window.attributes = window.attributes.also {
            it.rotationAnimation = WindowManager.LayoutParams.ROTATION_ANIMATION_JUMPCUT
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                it.preferMinimalPostProcessing = true
            }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            window.colorMode = ActivityInfo.COLOR_MODE_WIDE_COLOR_GAMUT
            // Push the device's actual display primaries down to Rust. `preferredWideGamutColorSpace` returns the ColorSpace.Rgb the panel claims (Display P3 on the Pixel 8 Pro). Its `transform` is the 9-float RGB→XYZ_D50 matrix in row-major order; `primaries` is `[Rx Ry Gx Gy Bx By]` in CIE 1931 xy chromaticity. Photon composes these with its own LMS→XYZ to drive the chromatic wave with native panel coordinates instead of a hardcoded REC2020 approximation that's outside the display's gamut anyway.
            val cs = display?.preferredWideGamutColorSpace
            if (cs is android.graphics.ColorSpace.Rgb) {
                nativeSetDisplayColorSpace(cs.transform, cs.primaries)
            } else {
                PhotonLog.w(TAG, "ColorPipeline: display.preferredWideGamutColorSpace is not Rgb (${cs?.javaClass?.simpleName}) — falling back to hardcoded matrices")
            }
        }
        // Initialize gravity sensor — listener fills gravityX/Y/Z and calls updateOrientationFromGravity at ~60Hz (SENSOR_DELAY_UI). gravitySensor may be null on very old devices (TYPE_GRAVITY is a virtual sensor composited from accelerometer + gyro); when null the listener never registers and orientation stays at whatever the OS picks.
        sensorManager = getSystemService(Context.SENSOR_SERVICE) as SensorManager
        gravitySensor = sensorManager.getDefaultSensor(Sensor.TYPE_GRAVITY)

        // Initialize scale gesture detector for pinch-to-zoom
        scaleGestureDetector = ScaleGestureDetector(this, object : ScaleGestureDetector.OnScaleGestureListener {
            override fun onScale(detector: ScaleGestureDetector): Boolean {
                if (nativePtr != 0L) {
                    nativeOnScale(nativePtr, detector.scaleFactor)
                }
                return true
            }

            override fun onScaleBegin(detector: ScaleGestureDetector): Boolean {
                return true  // Accept gesture
            }

            override fun onScaleEnd(detector: ScaleGestureDetector) {
                // No-op
            }
        })

        // Handle touch events with action types
        surfaceView.setOnTouchListener { _, event ->
            // Pass to scale detector first
            scaleGestureDetector.onTouchEvent(event)

            if (nativePtr != 0L) {
                val action = when (event.action and MotionEvent.ACTION_MASK) {
                    MotionEvent.ACTION_DOWN -> 0
                    MotionEvent.ACTION_UP -> 1
                    MotionEvent.ACTION_MOVE -> 2
                    MotionEvent.ACTION_CANCEL -> 3
                    else -> -1
                }
                if (action >= 0) {
                    val keyboardAction = nativeOnTouch(nativePtr, action, event.x, event.y)
                    when (keyboardAction) {
                        1 -> showKeyboard()
                        -1 -> hideKeyboard()
                    }
                }
            }
            true
        }

        // Listen for keyboard visibility changes via insets
        ViewCompat.setOnApplyWindowInsetsListener(surfaceView) { view, insets ->
            val imeVisible = insets.isVisible(WindowInsetsCompat.Type.ime())
            val imeHeight = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom

            if (imeVisible && !keyboardVisible) {
                keyboardVisible = true
                // Keyboard appeared - resize SurfaceView to visible area
                val visibleHeight = fullHeight - imeHeight
                if (visibleHeight > 0) {
                    val params = view.layoutParams as FrameLayout.LayoutParams
                    params.height = visibleHeight
                    view.layoutParams = params
                }
            } else if (!imeVisible && keyboardVisible) {
                keyboardVisible = false
                // Keyboard hidden - restore full height
                val params = view.layoutParams as FrameLayout.LayoutParams
                params.height = FrameLayout.LayoutParams.MATCH_PARENT
                view.layoutParams = params
            }
            insets
        }

        // Gather device fingerprint (no permissions needed)
        fingerprintResult = DeviceFingerprint.gather(this)
        initializeNativeIfReady()

        // Subscribe to FCM topic for peer update notifications
        // When any peer's IP changes, FGTW broadcasts to this topic
        FirebaseMessaging.getInstance().subscribeToTopic("peer_updates")

        // Request notification permission (Android 13+) and create channel
        requestNotificationPermission()
    }

    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            when {
                ContextCompat.checkSelfPermission(
                    this,
                    Manifest.permission.POST_NOTIFICATIONS
                ) == PackageManager.PERMISSION_GRANTED -> {
                    // Already have permission
                    createNotificationChannel()
                }
                else -> {
                    // Request permission
                    notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
            }
        } else {
            // Android 12 and below don't need runtime permission
            createNotificationChannel()
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val importance = NotificationManager.IMPORTANCE_HIGH
            val channel = NotificationChannel(CHANNEL_ID, CHANNEL_NAME, importance).apply {
                description = "Photon message notifications"
                // Silent channel: the app plays the sender's chirp (AudioTrack) + haptic (VibrationEffect)
                // itself, so the OS default tone never fires and the sound is per-contact. See
                // PhotonConnectionService.postMessageNotification.
                setSound(null, null)
                enableVibration(false)
            }
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(channel)
        }

        // Start foreground service for persistent connection
        startConnectionService()
    }

    private fun startConnectionService() {
        fingerprintResult?.let { fp ->
            // Start service with fingerprint and data dir
            val serviceIntent = Intent(this, PhotonConnectionService::class.java).apply {
                putExtra("fingerprint", fp.fingerprint)
                putExtra("dataDir", filesDir.absolutePath)
                // Shadow ring for the dual-ring vault — different mount than filesDir, so a single-volume torn write or partition flake doesn't take both rings down in the same session. getExternalFilesDir(null) is app-scoped (no permission needed, dies with uninstall just like filesDir) and on a real device is effectively always present; the Rust side falls back to filesDir-with-shadow-suffix if it ever comes back null.
                putExtra("shadowDir", getExternalFilesDir(null)?.absolutePath ?: "")
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                startForegroundService(serviceIntent)
            } else {
                startService(serviceIntent)
            }

            // Bind to service to get network pointer
            bindService(serviceIntent, serviceConnection, Context.BIND_AUTO_CREATE)
            PhotonLog.d(TAG, "Started and binding to PhotonConnectionService")
        }
    }

    private fun stopConnectionService() {
        if (serviceBound) {
            unbindService(serviceConnection)
            serviceBound = false
        }
        val serviceIntent = Intent(this, PhotonConnectionService::class.java)
        stopService(serviceIntent)
    }

    private fun initializeNativeIfReady() {
        // Need: surface ready, service bound with network initialized, not already initialized
        if (!surfaceReady || !serviceBound || nativePtr != 0L) return

        val service = connectionService ?: return
        if (!service.isNetworkReady()) {
            PhotonLog.d(TAG, "Service not ready yet, waiting...")
            return
        }

        val networkPtr = service.getNetworkPtr()
        if (networkPtr == 0L) {
            PhotonLog.e(TAG, "Network pointer is null")
            return
        }

        // On first launch after reinstall the tohu session file is gone but the sticky broadcast
        // may still hold the capsule. Restore before initializing the native UI so query_resume
        // can skip re-attest on the first frame.
        val broadcastVsf = service.readSessionBroadcast()
        if (broadcastVsf != null && broadcastVsf.isNotEmpty()) {
            val restored = nativeRestoreSessionFromVsf(broadcastVsf)
            PhotonLog.d(TAG, "Session restore from sticky broadcast: $restored")
        }

        val holder = surfaceView.holder
        // Samsung needs workarounds for Choreographer throttling
        val isSamsung = android.os.Build.MANUFACTURER.equals("samsung", ignoreCase = true)

        PhotonLog.d(TAG, "Initializing native UI with network ptr 0x${networkPtr.toString(16)}")
        nativePtr = nativeInitWithNetwork(
            holder.surfaceFrame.width(),
            holder.surfaceFrame.height(),
            networkPtr,
            isSamsung
        )

        if (nativePtr != 0L) {
            PhotonLog.d(TAG, "Native UI initialized at 0x${nativePtr.toString(16)}")
            // Hand the Activity-context ptr to the service so its RX worker can drive a headless
            // protocol tick (nativeServiceTick) while we're backgrounded — the Choreographer stops
            // calling doFrame then, but CLUTCH/chat still needs to advance. The ptr stays valid until
            // onDestroy, which clears it back to 0 so the service never ticks a freed context.
            service.setActivityContextPtr(nativePtr)
            // Start render loop
            Choreographer.getInstance().postFrameCallback(this)
        } else {
            PhotonLog.e(TAG, "Failed to initialize native UI")
        }
    }

    // SurfaceHolder.Callback
    override fun surfaceCreated(holder: SurfaceHolder) {
        surfaceReady = true
        // Capture full height on first surface creation (before any keyboard)
        if (fullHeight == 0) {
            fullHeight = holder.surfaceFrame.height()
        }
        initializeNativeIfReady()

        // Resume render loop if we already have a context (app resume case)
        if (nativePtr != 0L) {
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        // Track full height only when keyboard is not visible AND height is larger (prevents capturing reduced height if surfaceChanged races with keyboard).
        if (!keyboardVisible && height > fullHeight) {
            fullHeight = height
        }
        if (nativePtr != 0L) {
            nativeResize(nativePtr, width, height)
        }
    }

    private fun showKeyboard() {
        val imm = getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager
        surfaceView.requestFocus()
        imm.showSoftInput(surfaceView, InputMethodManager.SHOW_IMPLICIT)
    }

    private fun hideKeyboard() {
        val imm = getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager
        imm.hideSoftInputFromWindow(surfaceView.windowToken, 0)
    }

    // Reset the IME's input state on the surface — recreates the InputConnection (composingText = ""), so a predictive keyboard forgets the text we just sent and cleared.
    private fun restartImeInput() {
        val imm = getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager
        imm.restartInput(surfaceView)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        surfaceReady = false
        Choreographer.getInstance().removeFrameCallback(this)
    }

    // Choreographer.FrameCallback - render loop
    override fun doFrame(frameTimeNanos: Long) {
        if (nativePtr != 0L && surfaceReady) {
            val surface = surfaceView.holder.surface
            if (surface.isValid) {
                nativeDraw(nativePtr, surface)
                // Poll the soft-keyboard signal each frame so app-driven focus changes (e.g. dropping focus from the textbox when attestation starts) propagate to the IME without waiting for the next user touch. `wants_keyboard` is a take-on-change one-shot — almost every frame returns 0 (no change), so this is cheap.
                when (nativePollKeyboard(nativePtr)) {
                    1 -> showKeyboard()
                    -1 -> hideKeyboard()
                }
                // After a send the app cleared its compose box; restart IME input so a predictive keyboard's stale composing buffer doesn't re-materialise the just-sent text on the next keystroke.
                if (nativePollInputReset(nativePtr) == 1) {
                    restartImeInput()
                }
                // Avatar tap (Ready screen) → launch the system image picker. App-driven, not touch-driven, so it polls alongside the keyboard signal instead of riding the nativeOnTouch return like the legacy 2-code path did.
                if (nativePollAvatarPicker(nativePtr) == 1) {
                    openImagePicker()
                }
                when (nativePollSessionBroadcast(nativePtr)) {
                    1 -> connectionService?.sendSessionBroadcast()
                    -1 -> connectionService?.clearSessionBroadcast()
                }
                // Self-update: a hash-verified APK is staged — hand it to the system installer (the OS owns package installs; its prompt is the second click).
                nativePollApkInstall(nativePtr)?.let { apkPath ->
                    installApk(apkPath)
                }
                // Clipboard hand-off (copy-words / copy-name): Android 13+ shows its own "copied" overlay.
                nativePollClipboardCopy(nativePtr)?.let { text ->
                    val cm = getSystemService(android.content.ClipboardManager::class.java)
                    cm.setPrimaryClip(android.content.ClipData.newPlainText("photon", text))
                }
            }
            // Schedule next frame
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    /** Install a staged self-update APK (docs/updates.md, Android path). PackageInstaller SESSION first: on Android 12+ with photon as its own installer-of-record the install is UNATTENDED (USER_ACTION_NOT_REQUIRED) — no dialog, the OS swaps the package and restarts us. The first session install (or an OEM that refuses unattended) surfaces the one system confirm via STATUS_PENDING_USER_ACTION, which is also the bootstrap that TRANSFERS installer-of-record to photon — silent from then on. The classic ACTION_VIEW intent stays as the last-ditch fallback. */
    private fun installApk(path: String) {
        try {
            installApkSession(path)
        } catch (e: Exception) {
            PhotonLog.e("Update", "session install failed (${e.message}) — falling back to system installer intent")
            installApkIntent(path)
        }
    }

    private fun installApkSession(path: String) {
        val installer = packageManager.packageInstaller
        val params = android.content.pm.PackageInstaller.SessionParams(
            android.content.pm.PackageInstaller.SessionParams.MODE_FULL_INSTALL
        ).apply {
            setAppPackageName(packageName)
            if (android.os.Build.VERSION.SDK_INT >= 31) {
                setRequireUserAction(android.content.pm.PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
            }
        }
        val sessionId = installer.createSession(params)
        installer.openSession(sessionId).use { session ->
            java.io.File(path).inputStream().use { input ->
                session.openWrite("photon-update.apk", 0, -1).use { out ->
                    input.copyTo(out)
                    session.fsync(out)
                }
            }
            // MUTABLE on purpose: the installer fills EXTRA_INTENT/EXTRA_STATUS into this PendingIntent.
            val intent = android.content.Intent(this, PhotonInstallReceiver::class.java).apply {
                action = "com.photon.messenger.INSTALL_RESULT"
            }
            val flags = if (android.os.Build.VERSION.SDK_INT >= 31) {
                android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_MUTABLE
            } else {
                android.app.PendingIntent.FLAG_UPDATE_CURRENT
            }
            val pi = android.app.PendingIntent.getBroadcast(this, sessionId, intent, flags)
            PhotonLog.i("Update", "committing install session $sessionId (unattended where permitted)")
            session.commit(pi.intentSender)
        }
    }

    private fun installApkIntent(path: String) {
        try {
            val file = java.io.File(path)
            val uri = androidx.core.content.FileProvider.getUriForFile(this, "$packageName.fileprovider", file)
            val intent = android.content.Intent(android.content.Intent.ACTION_VIEW).apply {
                setDataAndType(uri, "application/vnd.android.package-archive")
                addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
                addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            startActivity(intent)
        } catch (e: Exception) {
            PhotonLog.e("Update", "installer launch failed: ${e.message}")
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        Choreographer.getInstance().removeFrameCallback(this)
        if (nativePtr != 0L) {
            // Retract the ptr from the service FIRST so its RX worker can't fire a headless tick into
            // a context we're about to free, then destroy.
            connectionService?.setActivityContextPtr(0L)
            nativeDestroy(nativePtr)
            nativePtr = 0
        }
    }

    override fun onPause() {
        super.onPause()
        inForeground = false
        Choreographer.getInstance().removeFrameCallback(this)
        sensorManager.unregisterListener(gravityListener)
        contentResolver.unregisterContentObserver(rotationSettingObserver)
    }

    override fun onResume() {
        super.onResume()
        inForeground = true
        // Entering the app clears any pending "new message" notification — the user is now looking at the message list.
        getSystemService(NotificationManager::class.java)
            ?.cancel(PhotonConnectionService.MESSAGE_NOTIFICATION_ID)
        gravitySensor?.let {
            sensorManager.registerListener(gravityListener, it, SensorManager.SENSOR_DELAY_UI)
        }
        // Re-read the auto-rotate lock (the user may have toggled it while we were backgrounded) and watch it live.
        autoRotateEnabled = readAutoRotate()
        updateOrientationFromGravity()
        contentResolver.registerContentObserver(
            Settings.System.getUriFor(Settings.System.ACCELEROMETER_ROTATION),
            false,
            rotationSettingObserver,
        )
        if (nativePtr != 0L && surfaceReady) {
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    // Handle key events from soft keyboard
    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (nativePtr == 0L) return super.onKeyDown(keyCode, event)

        // Handle special keys
        when (keyCode) {
            KeyEvent.KEYCODE_DEL,      // Backspace
            KeyEvent.KEYCODE_ENTER,    // Enter/Done
            KeyEvent.KEYCODE_DPAD_LEFT,
            KeyEvent.KEYCODE_DPAD_RIGHT -> {
                if (nativeOnKeyEvent(nativePtr, keyCode)) {
                    return true
                }
            }
        }

        // 3-button-nav back key: claim it so we can distinguish a LONG press (the real close — Shift+Escape's desktop twin) from a tap (one back level / hide). Gesture navigation never delivers this key — it arrives via onBackPressed with no long-press concept, so gesture users only get tap semantics.
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            event?.startTracking()
            return true
        }

        // Handle text input
        event?.let {
            val unicodeChar = it.unicodeChar
            if (unicodeChar != 0) {
                val text = unicodeChar.toChar().toString()
                nativeOnTextInput(nativePtr, text)
                return true
            }
        }

        return super.onKeyDown(keyCode, event)
    }

    override fun onKeyLongPress(keyCode: Int, event: KeyEvent?): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            // Long-press back = the deliberate quit (desktop's Shift+Escape): remove the task, but the foreground service stays — the network + doorbell survive; only the UI is gone.
            PhotonLog.d(TAG, "Back long-press — real close (task removed, service stays)")
            finishAndRemoveTask()
            return true
        }
        return super.onKeyLongPress(keyCode, event)
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent?): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            // A completed TAP (not canceled by the long-press firing): normal back semantics.
            if (event != null && !event.isCanceled) {
                backOneLevel()
            }
            return true
        }
        return super.onKeyUp(keyCode, event)
    }

    // One back step: Rust pops a level (friend panel → chat → contacts, settings → contacts, …); at the top of the stack it declines and we HIDE, never exit — the app is resident (foreground service keeps the network alive; desktop parity is close-to-tray). The Activity stays warm for an instant resume.
    private fun backOneLevel() {
        if (nativePtr != 0L && nativeOnBackPressed(nativePtr)) {
            return
        }
        moveTaskToBack(true)
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        // Gesture-nav back (no key events) lands here.
        backOneLevel()
    }
}
