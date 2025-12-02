package com.photon.messenger

import android.graphics.PixelFormat
import android.os.Bundle
import android.view.Choreographer
import android.view.KeyEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.google.firebase.messaging.FirebaseMessaging

class PhotonActivity : AppCompatActivity(), SurfaceHolder.Callback, Choreographer.FrameCallback {

    companion object {
        init {
            System.loadLibrary("photon_messenger")
        }
    }

    // Native context pointer
    private var nativePtr: Long = 0

    // Surface for rendering (custom class with InputConnection)
    private lateinit var surfaceView: PhotonSurfaceView
    private var surfaceReady = false

    // Device fingerprint result
    private var fingerprintResult: DeviceFingerprint.FingerprintResult? = null

    // Track full screen height (before keyboard)
    private var fullHeight: Int = 0
    private var keyboardVisible = false

    // Native methods
    private external fun nativeInit(width: Int, height: Int, fingerprint: ByteArray, dataDir: String, isSamsung: Boolean): Long
    private external fun nativeDraw(contextPtr: Long, surface: android.view.Surface)
    private external fun nativeResize(contextPtr: Long, width: Int, height: Int)
    private external fun nativeOnTouch(contextPtr: Long, action: Int, x: Float, y: Float): Int  // Returns: 1=show keyboard, -1=hide keyboard, 2=open image picker, 0=no change
    private external fun nativeOnTextInput(contextPtr: Long, text: String)  // Text from soft keyboard
    private external fun nativeOnKeyEvent(contextPtr: Long, keyCode: Int): Boolean  // Special keys (backspace, enter)
    private external fun nativeOnBackPressed(contextPtr: Long): Boolean  // Back button - returns true if handled
    private external fun nativeSetAvatarFromFile(contextPtr: Long, fileBytes: ByteArray)  // Raw image file bytes (preserves ICC profile)
    private external fun nativeDestroy(contextPtr: Long)

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

        // Handle touch events with action types
        surfaceView.setOnTouchListener { _, event ->
            if (nativePtr != 0L) {
                val action = when (event.action) {
                    android.view.MotionEvent.ACTION_DOWN -> 0
                    android.view.MotionEvent.ACTION_UP -> 1
                    android.view.MotionEvent.ACTION_MOVE -> 2
                    android.view.MotionEvent.ACTION_CANCEL -> 3
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
    }

    private fun initializeNativeIfReady() {
        if (surfaceReady && fingerprintResult != null && nativePtr == 0L) {
            val holder = surfaceView.holder
            // Samsung needs workarounds for Choreographer throttling
            val isSamsung = android.os.Build.MANUFACTURER.equals("samsung", ignoreCase = true)
            nativePtr = nativeInit(
                holder.surfaceFrame.width(),
                holder.surfaceFrame.height(),
                fingerprintResult!!.fingerprint,
                filesDir.absolutePath,
                isSamsung
            )

            if (nativePtr != 0L) {
                // Start render loop
                Choreographer.getInstance().postFrameCallback(this)
            }
        }
    }

    // SurfaceHolder.Callback
    override fun surfaceCreated(holder: SurfaceHolder) {
        surfaceReady = true
        initializeNativeIfReady()

        // Resume render loop if we already have a context (app resume case)
        if (nativePtr != 0L) {
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        // Track full height when keyboard is not visible
        if (!keyboardVisible) {
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
