package com.photon.messenger

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Log
import androidx.core.app.NotificationCompat

/**
 * Foreground service that owns Photon's network stack.
 *
 * The network context (HandleQuery, FgtwTransport) lives here and persists
 * across Activity lifecycle changes. The Activity binds to this service
 * to get the network context pointer for rendering.
 *
 * Architecture:
 * - Service: Owns network stack, runs background polling thread
 * - Activity: Owns UI renderer, binds to service for network access
 */
class PhotonConnectionService : Service() {

    companion object {
        init {
            System.loadLibrary("photon_messenger")
        }
        const val CHANNEL_ID = "photon_connection"
        const val NOTIFICATION_ID = 1001
        const val MESSAGE_NOTIFICATION_ID = 1002
        private const val TAG = "PhotonService"
        private const val POLL_INTERVAL_MS = 1000L // 1 second network polling
        const val SESSION_ACTION = "com.photon.SESSION"
        const val SESSION_PERMISSION = "com.photon.SESSION_READ"
        const val SESSION_EXTRA_VSF = "vsf"
    }

    // Native network context pointer (HandleQuery + FgtwTransport)
    private var networkPtr: Long = 0

    // Device keypair info for display
    private var devicePubkeyHex: String = ""

    // Background thread for network polling
    private var networkThread: HandlerThread? = null
    private var networkHandler: Handler? = null
    private var isPolling = false

    // Binder for Activity to get network pointer
    private val binder = LocalBinder()

    inner class LocalBinder : Binder() {
        fun getService(): PhotonConnectionService = this@PhotonConnectionService
    }

    // Native methods for network operations
    private external fun nativeNetworkInit(fingerprint: ByteArray, dataDir: String, shadowDir: String): Long
    private external fun nativeNetworkDestroy(networkPtr: Long)
    private external fun nativeNetworkPoll(networkPtr: Long)  // Check for incoming messages, refresh peers
    private external fun nativeGetDevicePubkey(networkPtr: Long): String

    // Session broadcast — VSF capsule carrying identity_seed + vault_seed + handle_proof.
    // Sticky broadcast survives uninstall/reinstall (OS holds it); dies on reboot (desired).
    // Seeds never leave Rust — nativeSendSessionBroadcast reads tohu::session() internally.
    private external fun nativeSendSessionBroadcast(context: android.content.Context)
    private external fun nativeClearSessionBroadcast(context: android.content.Context)


    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        Log.d(TAG, "Service created")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Extract fingerprint and data dirs from intent. shadowDir is "" if getExternalFilesDir returned null on the Activity side — Rust treats empty as "fall back to dataDir with shadow-suffix filename".
        val fingerprint = intent?.getByteArrayExtra("fingerprint")
        val dataDir = intent?.getStringExtra("dataDir")
        val shadowDir = intent?.getStringExtra("shadowDir") ?: ""

        if (fingerprint != null && dataDir != null && networkPtr == 0L) {
            // Initialize network stack
            networkPtr = nativeNetworkInit(fingerprint, dataDir, shadowDir)
            if (networkPtr != 0L) {
                devicePubkeyHex = nativeGetDevicePubkey(networkPtr)
                Log.d(TAG, "Network initialized, device: ${devicePubkeyHex.take(16)}...")
                startNetworkPolling()
            } else {
                Log.e(TAG, "Failed to initialize network")
            }
        }

        val notification = buildNotification()
        startForeground(NOTIFICATION_ID, notification)
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        Log.d(TAG, "Service destroying")
        stopNetworkPolling()
        if (networkPtr != 0L) {
            nativeNetworkDestroy(networkPtr)
            networkPtr = 0
        }
        super.onDestroy()
    }

    /**
     * Broadcast the session capsule as a sticky broadcast so it survives reinstall.
     * Called after successful attestation. Rust reads the seeds from tohu::session() — they
     * never surface in Kotlin.
     */
    fun sendSessionBroadcast() {
        nativeSendSessionBroadcast(this)
    }

    /**
     * Remove the sticky session broadcast. Called on logout / vault nuke.
     */
    fun clearSessionBroadcast() {
        nativeClearSessionBroadcast(this)
        Log.d(TAG, "Session broadcast cleared")
    }

    /**
     * Read the sticky session broadcast from the OS (query without registering a receiver).
     * Returns the raw VSF bytes, or null if no broadcast is present.
     */
    fun readSessionBroadcast(): ByteArray? {
        val filter = android.content.IntentFilter(SESSION_ACTION)
        @Suppress("UnspecifiedRegisterReceiverFlag")
        val intent = registerReceiver(null, filter) ?: return null
        return intent.getByteArrayExtra(SESSION_EXTRA_VSF)
    }

    /**
     * Get the native network context pointer for the Activity to use.
     * Returns 0 if not initialized yet.
     */
    fun getNetworkPtr(): Long = networkPtr

    /**
     * Check if network is initialized and ready.
     */
    fun isNetworkReady(): Boolean = networkPtr != 0L

    private fun startNetworkPolling() {
        if (isPolling) return
        isPolling = true

        networkThread = HandlerThread("PhotonNetwork").apply { start() }
        networkHandler = Handler(networkThread!!.looper)

        val pollRunnable = object : Runnable {
            override fun run() {
                if (networkPtr != 0L && isPolling) {
                    nativeNetworkPoll(networkPtr)
                    networkHandler?.postDelayed(this, POLL_INTERVAL_MS)
                }
            }
        }
        networkHandler?.post(pollRunnable)
        Log.d(TAG, "Network polling started")
    }

    private fun stopNetworkPolling() {
        isPolling = false
        networkHandler?.removeCallbacksAndMessages(null)
        networkThread?.quitSafely()
        networkThread = null
        networkHandler = null
        Log.d(TAG, "Network polling stopped")
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Connection Status",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Shows when Photon is maintaining a secure connection"
                setShowBadge(false)
            }
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(): Notification {
        val pendingIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, PhotonActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE
        )

        val statusText = if (networkPtr != 0L) {
            "Secure connection active"
        } else {
            "Connecting..."
        }

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Photon")
            .setContentText(statusText)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    /**
     * Post the "new message" notification, sounding + buzzing the SENDER's per-contact chirp. Called
     * FROM RUST (the status RX worker, via the global-ref registered in nativeNetworkInit) on any
     * thread. Suppressed while the Activity is visible (the user is already looking at the app; in-app
     * presentation owns that case).
     *
     * The channel is SILENT (no channel sound/vibration) — this method plays the sound and haptic
     * ITSELF: [wav] (mono 16-bit PCM the Rust chirp crate rendered) via AudioTrack, and [timings] /
     * [amplitudes] (the matching amplitude envelope) via VibrationEffect.createWaveform. That's the
     * "app plays it after wake" path: the tone is per-contact, the OS default never fires, and it works
     * even from deep Doze once the process is scheduled. A fixed notification id collapses a burst into
     * one entry. No content beyond "New message": plaintext never reaches this layer and the handle
     * stays off the lock screen; only the rendered audio/haptic came from Rust, never the sender key.
     */
    fun postMessageNotification(wav: ByteArray, timings: LongArray, amplitudes: IntArray) {
        if (PhotonActivity.inForeground) return

        // The Activity normally creates the silent channel first, but be self-sufficient: creation is
        // idempotent, and it MUST match the Activity's silent definition (setSound(null)/no vibration)
        // — whichever creates it first wins, and its sound/vibration are then immutable.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                PhotonActivity.CHANNEL_ID,
                PhotonActivity.CHANNEL_NAME,
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = "Photon message notifications"
                setSound(null, null)
                enableVibration(false)
            }
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }

        val pendingIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, PhotonActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE
        )
        val notification = NotificationCompat.Builder(this, PhotonActivity.CHANNEL_ID)
            .setContentTitle("Photon")
            .setContentText("New message")
            .setSmallIcon(android.R.drawable.ic_dialog_email)
            .setContentIntent(pendingIntent)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .build()
        getSystemService(NotificationManager::class.java).notify(MESSAGE_NOTIFICATION_ID, notification)

        playChirp(wav)
        vibrateChirp(timings, amplitudes)
    }

    /** Play the chirp WAV (mono 16-bit PCM) on the NOTIFICATION audio stream via AudioTrack. Fire-and-
     *  forget on the service's own thread; any failure (no output route, malformed clip) is logged, never
     *  fatal — a missing sound must not take the service down. The 44-byte WAV header is skipped; the rest
     *  is streamed as PCM16. Sample rate is read from the header so it tracks the chirp crate's rate. */
    private fun playChirp(wav: ByteArray) {
        if (wav.size <= 44) return
        try {
            // Little-endian sample rate lives at header offset 24..27.
            val sampleRate = (wav[24].toInt() and 0xFF) or
                ((wav[25].toInt() and 0xFF) shl 8) or
                ((wav[26].toInt() and 0xFF) shl 16) or
                ((wav[27].toInt() and 0xFF) shl 24)
            val pcm = wav.copyOfRange(44, wav.size)

            val attrs = AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_NOTIFICATION)
                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                .build()
            val format = AudioFormat.Builder()
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setSampleRate(sampleRate)
                .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                .build()
            val track = AudioTrack(
                attrs, format, pcm.size,
                AudioTrack.MODE_STATIC, AudioManager.AUDIO_SESSION_ID_GENERATE
            )
            track.write(pcm, 0, pcm.size)
            track.setNotificationMarkerPosition(pcm.size / 2) // 2 bytes/frame (mono 16-bit)
            track.setPlaybackPositionUpdateListener(object : AudioTrack.OnPlaybackPositionUpdateListener {
                override fun onMarkerReached(t: AudioTrack) { t.release() }
                override fun onPeriodicNotification(t: AudioTrack) {}
            })
            track.play()
        } catch (e: Exception) {
            Log.w(TAG, "playChirp failed", e)
        }
    }

    /** Fire the chirp's amplitude-envelope haptic via VibrationEffect.createWaveform (API 26+). timings
     *  are per-step durations (ms), amplitudes are 0..255 motor levels — the pair the chirp crate's
     *  haptic_waveform produced. -1 = no repeat. Logged-not-fatal on any failure. */
    private fun vibrateChirp(timings: LongArray, amplitudes: IntArray) {
        if (timings.isEmpty() || timings.size != amplitudes.size) return
        try {
            val vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                (getSystemService(VibratorManager::class.java)).defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                getSystemService(Vibrator::class.java)
            }
            if (vibrator == null || !vibrator.hasVibrator()) return
            val effect = VibrationEffect.createWaveform(timings, amplitudes, -1)
            vibrator.vibrate(effect)
        } catch (e: Exception) {
            Log.w(TAG, "vibrateChirp failed", e)
        }
    }
}
