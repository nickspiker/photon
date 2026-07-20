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
import android.os.PowerManager
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
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

        // The live service instance, for the FCM doorbell path (PhotonMessagingService needs to poke
        // a protocol tick on wake, and Android gives it no binding to an already-running service).
        // Set in onCreate, cleared in onDestroy; @Volatile because FCM delivers on a binder thread.
        @Volatile var live: PhotonConnectionService? = null
    }

    // Native network context pointer (HandleQuery + FgtwTransport)
    private var networkPtr: Long = 0

    // The Activity-side native context ptr (fluor shell + PhotonApp), handed to us by the Activity in
    // initializeNativeIfReady and retracted (0) in onDestroy. Lets the RX worker drive a headless
    // protocol tick while the Activity is backgrounded. @Volatile: written on the main thread, read on
    // the RX worker thread. 0 = no live Activity context → skip the tick. See docs/background-tick.md.
    @Volatile private var activityContextPtr: Long = 0

    // Brief wakelock held only across a headless tick, so the CPU is scheduled to run advance_protocol
    // when a packet arrives while the screen is off. Acquired with a short timeout as a safety net.
    private val wakeLock: PowerManager.WakeLock by lazy {
        (getSystemService(POWER_SERVICE) as PowerManager)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "photon:serviceTick")
            .apply { setReferenceCounted(false) }
    }

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
    private external fun nativeSetFcmToken(token: String, projectId: String)
    private external fun nativeNetworkDestroy(networkPtr: Long)
    private external fun nativeNetworkPoll(networkPtr: Long)  // Check for incoming messages, refresh peers
    private external fun nativeGetDevicePubkey(networkPtr: Long): String
    private external fun nativeServiceTick(contextPtr: Long)  // Headless advance_protocol on the Activity ctx (background delivery)

    // Session broadcast — VSF capsule carrying identity_seed + vault_seed + handle_proof.
    // Sticky broadcast survives uninstall/reinstall (OS holds it); dies on reboot (desired).
    // Seeds never leave Rust — nativeSendSessionBroadcast reads tohu::session() internally.
    private external fun nativeSendSessionBroadcast(context: android.content.Context)
    private external fun nativeClearSessionBroadcast(context: android.content.Context)


    override fun onCreate() {
        super.onCreate()
        live = this
        createNotificationChannel()
        PhotonLog.d(TAG, "Service created")
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
                PhotonLog.d(TAG, "Network initialized, device: ${devicePubkeyHex.take(16)}...")
                startNetworkPolling()
                // Hand the FCM bell material to Rust: the ping cycle publishes `fcm:<project>:<token>`
                // to the worker's bell registry so a sender can wake this phone from deep Doze
                // (docs/reachability-doorbell.md). Project id comes off the baked google-services.json
                // — a fork's tenant flows thru without code changes. Rotation lands via onNewToken.
                try {
                    val projectId = com.google.firebase.FirebaseApp.getInstance().options.projectId ?: ""
                    if (projectId.isNotEmpty()) {
                        com.google.firebase.messaging.FirebaseMessaging.getInstance().token
                            .addOnSuccessListener { token ->
                                if (!token.isNullOrEmpty()) nativeSetFcmToken(token, projectId)
                            }
                        // Release-notice fan-out: one topic send from the worker reaches every subscriber (docs/reachability-doorbell.md + updates.md). Subscription is idempotent.
                        com.google.firebase.messaging.FirebaseMessaging.getInstance().subscribeToTopic("updates")
                    }
                } catch (e: Exception) {
                    PhotonLog.w(TAG, "FCM token fetch failed (no Play services?)", e)
                }
            } else {
                PhotonLog.e(TAG, "Failed to initialize network")
            }
        }

        val notification = buildNotification()
        startForeground(NOTIFICATION_ID, notification)
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        live = null
        PhotonLog.d(TAG, "Service destroying")
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
        PhotonLog.d(TAG, "Session broadcast cleared")
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

    /**
     * The Activity hands us its native context ptr once the fluor shell + PhotonApp exist, and retracts
     * it (0) in onDestroy before freeing. We only ever tick a ptr the Activity currently vouches is live.
     */
    fun setActivityContextPtr(ptr: Long) {
        activityContextPtr = ptr
    }

    /**
     * Called FROM RUST (the status RX worker, via the service global-ref) whenever an inbound StatusUpdate
     * lands, so the protocol advances (CLUTCH ceremony, chain, ACKs) even while the Activity is
     * backgrounded and its Choreographer has stopped calling tick. Grabs a brief PARTIAL_WAKE_LOCK so the
     * CPU is scheduled to run the tick, drives the headless advance_protocol via nativeServiceTick, then
     * releases. No-op if the Activity context isn't set (destroyed / not yet created) — that native side
     * also skips if a foreground draw is concurrently in progress (the onResume overlap). Any thread.
     * See docs/background-tick.md.
     */
    fun requestServiceTick() {
        val ptr = activityContextPtr
        if (ptr == 0L) return  // no live Activity context (destroyed / not yet created)
        try {
            wakeLock.acquire(2_000L)  // safety-net timeout; the tick is milliseconds
            nativeServiceTick(ptr)
        } catch (e: Exception) {
            PhotonLog.w(TAG, "requestServiceTick failed", e)
        } finally {
            if (wakeLock.isHeld) wakeLock.release()
        }
    }

    private fun startNetworkPolling() {
        if (isPolling) return
        isPolling = true

        networkThread = HandlerThread("PhotonNetwork").apply { start() }
        networkHandler = Handler(networkThread!!.looper)

        val pollRunnable = object : Runnable {
            override fun run() {
                if (networkPtr != 0L && isPolling) {
                    nativeNetworkPoll(networkPtr)
                    // While backgrounded, the Activity's Choreographer has stopped calling tick, so the
                    // protocol (presence pings, CLUTCH ceremony, chain/ACK) would stall. Drive a headless
                    // advance_protocol from this self-scheduled poll instead — it keeps pinging (so pongs
                    // keep arriving) and drains inbound traffic without the screen on. Foregrounded, the
                    // live draw already does this, so we skip (the native guard would make us skip anyway).
                    // The RX-triggered requestServiceTick still fires on inbound packets for lower latency;
                    // this periodic drive is what keeps the send side alive so there's traffic to react to.
                    // See docs/background-tick.md.
                    if (!PhotonActivity.inForeground) {
                        requestServiceTick()
                    }
                    networkHandler?.postDelayed(this, POLL_INTERVAL_MS)
                }
            }
        }
        networkHandler?.post(pollRunnable)
        PhotonLog.d(TAG, "Network polling started")
    }

    private fun stopNetworkPolling() {
        isPolling = false
        networkHandler?.removeCallbacksAndMessages(null)
        networkThread?.quitSafely()
        networkThread = null
        networkHandler = null
        PhotonLog.d(TAG, "Network polling stopped")
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
            PhotonLog.w(TAG, "playChirp failed", e)
        }
    }

    /** Fire the chirp's amplitude-envelope haptic via VibrationEffect.createWaveform (API 26+). timings
     *  are per-step durations (ms), amplitudes are 0..255 motor levels — the pair the chirp crate's
     *  haptic_waveform produced. -1 = no repeat. Logged-not-fatal on any failure.
     *
     *  Usage attributes are load-bearing: a bare effect is usage=UNKNOWN and the OS drops it. Picking the
     *  usage is fiddly because each is gated by a DIFFERENT device setting: USAGE_NOTIFICATION is gated by
     *  our (deliberately silent) notification channel, and USAGE_TOUCH is gated by the touch-feedback
     *  setting (off by default on Pixel → ignored_for_settings). USAGE_COMMUNICATION_REQUEST is gated by
     *  none of those — it played on-device (dumpsys: finished, not ignored) when the others didn't — so the
     *  app-driven chirp haptic fires regardless of the silent channel and touch-feedback off.
     *
     *  The amplitude curve itself is shaped upstream in the chirp crate (mean-square power per bin,
     *  peak-normalized to 0..255); we hand it to the motor as-is, no remap. */
    private fun vibrateChirp(timings: LongArray, amplitudes: IntArray) {
        if (timings.isEmpty() || timings.size != amplitudes.size) {
            PhotonLog.w(TAG, "vibrateChirp: bad arrays t=${timings.size} a=${amplitudes.size}")
            return
        }
        try {
            val vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                (getSystemService(VibratorManager::class.java)).defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                getSystemService(Vibrator::class.java)
            }
            if (vibrator == null || !vibrator.hasVibrator()) {
                PhotonLog.w(TAG, "vibrateChirp: no vibrator")
                return
            }

            // The chirp crate already shapes the envelope (mean-square power per bin, peak-normalized to
            // 0..255) — feed it straight to the motor.
            val effect = VibrationEffect.createWaveform(timings, amplitudes, -1)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                // USAGE_COMMUNICATION_REQUEST: ungated by the silent notification channel AND by the
                // touch-feedback setting — the one usage that actually played on-device (see doc above).
                val attrs = VibrationAttributes.Builder()
                    .setUsage(VibrationAttributes.USAGE_COMMUNICATION_REQUEST)
                    .build()
                vibrator.vibrate(effect, attrs)
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(effect)
            }
            PhotonLog.i(TAG, "vibrateChirp: fired ${amplitudes.size} steps, peak=${amplitudes.max()}")
        } catch (e: Exception) {
            PhotonLog.w(TAG, "vibrateChirp failed", e)
        }
    }
}
