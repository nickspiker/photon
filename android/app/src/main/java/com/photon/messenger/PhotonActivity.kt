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
import android.graphics.PixelFormat
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.util.Log
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
        const val CHANNEL_ID = "photon_messages"
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

    /// Snap raw gravity to one of the four screen orientations with a 7.0 m/s² deadband (≈71% of g, well past the 45° diagonal). Below threshold on both horizontal axes (phone near-flat) the previous orientation is retained.
    private fun updateOrientationFromGravity() {
        val target = when {
            gravityY > 7.0f -> ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
            gravityX < -7.0f -> ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE
            gravityY < -7.0f -> ActivityInfo.SCREEN_ORIENTATION_REVERSE_PORTRAIT
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
            Log.d(TAG, "Bound to PhotonConnectionService")
            // Try to initialize UI now that service is ready
            initializeNativeIfReady()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            connectionService = null
            serviceBound = false
            Log.d(TAG, "Disconnected from PhotonConnectionService")
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
                Log.w(TAG, "ColorPipeline: display.preferredWideGamutColorSpace is not Rgb (${cs?.javaClass?.simpleName}) — falling back to hardcoded matrices")
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
                enableVibration(true)
                vibrationPattern = longArrayOf(0, 250, 100, 250)
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
            Log.d(TAG, "Started and binding to PhotonConnectionService")
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
            Log.d(TAG, "Service not ready yet, waiting...")
            return
        }

        val networkPtr = service.getNetworkPtr()
        if (networkPtr == 0L) {
            Log.e(TAG, "Network pointer is null")
            return
        }

        // On first launch after reinstall the tohu session file is gone but the sticky broadcast
        // may still hold the capsule. Restore before initializing the native UI so query_resume
        // can skip re-attest on the first frame.
        val broadcastVsf = service.readSessionBroadcast()
        if (broadcastVsf != null && broadcastVsf.isNotEmpty()) {
            val restored = nativeRestoreSessionFromVsf(broadcastVsf)
            Log.d(TAG, "Session restore from sticky broadcast: $restored")
        }

        val holder = surfaceView.holder
        // Samsung needs workarounds for Choreographer throttling
        val isSamsung = android.os.Build.MANUFACTURER.equals("samsung", ignoreCase = true)

        Log.d(TAG, "Initializing native UI with network ptr 0x${networkPtr.toString(16)}")
        nativePtr = nativeInitWithNetwork(
            holder.surfaceFrame.width(),
            holder.surfaceFrame.height(),
            networkPtr,
            isSamsung
        )

        if (nativePtr != 0L) {
            Log.d(TAG, "Native UI initialized at 0x${nativePtr.toString(16)}")
            // Start render loop
            Choreographer.getInstance().postFrameCallback(this)
        } else {
            Log.e(TAG, "Failed to initialize native UI")
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
            }
            // Schedule next frame
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        Choreographer.getInstance().removeFrameCallback(this)
        if (nativePtr != 0L) {
            nativeDestroy(nativePtr)
            nativePtr = 0
        }
    }

    override fun onPause() {
        super.onPause()
        inForeground = false
        Choreographer.getInstance().removeFrameCallback(this)
        sensorManager.unregisterListener(gravityListener)
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

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (nativePtr != 0L && nativeOnBackPressed(nativePtr)) {
            // Rust handled it (e.g., navigated from chat to contacts)
            return
        }
        // Not handled - let system handle (exit app)
        @Suppress("DEPRECATION")
        super.onBackPressed()
    }
}
