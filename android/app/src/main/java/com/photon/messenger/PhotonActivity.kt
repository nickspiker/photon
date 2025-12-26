package com.photon.messenger

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.graphics.PixelFormat
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
    private external fun nativeSetAvatarFromFile(contextPtr: Long, fileBytes: ByteArray)  // Raw image file bytes (preserves ICC profile)
    private external fun nativeDestroy(contextPtr: Long)

    // Notification permission request (Android 13+)
    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { isGranted ->
        if (isGranted) {
            createNotificationChannel()
        }
    }

    // Image picker for avatar selection - passes RAW FILE BYTES to Rust
    // We do NOT decode in Android because BitmapFactory destroys ICC profiles
    // and mangles colors. Rust handles proper color management via XYZ.
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
                        2 -> openImagePicker()
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
        // Track full height only when keyboard is not visible AND height is larger
        // (prevents capturing reduced height if surfaceChanged races with keyboard)
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
        Choreographer.getInstance().removeFrameCallback(this)
    }

    override fun onResume() {
        super.onResume()
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
