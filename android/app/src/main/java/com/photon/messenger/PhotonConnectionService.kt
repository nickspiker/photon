package com.photon.messenger

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
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
        // "_quiet" suffix (2026-07-19): channel importance is IMMUTABLE once created on a device, so demoting the old IMPORTANCE_LOW "photon_connection" channel to MIN required a fresh id. The old channel lingers orphaned on upgraded installs (harmless; Android hides channels with no notifications).
        const val CHANNEL_ID = "photon_connection_quiet"
        const val NOTIFICATION_ID = 1001
        const val MESSAGE_NOTIFICATION_ID = 1002
        const val CALL_NOTIFICATION_ID = 1003
        const val ACTION_ANSWER_CALL = "com.photon.ANSWER_CALL"
        const val ACTION_DECLINE_CALL = "com.photon.DECLINE_CALL"
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
    private external fun nativeNetworkInit(fingerprint: ByteArray, dataDir: String, shadowDir: String, bootSecret: ByteArray): Long
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
    private external fun nativeCallAction(answer: Boolean)  // Answer/Decline from the call notification → Rust's pending-action latch (the app tick drains it)
    private external fun nativeAudioRoute(kind: Int, id: String)  // Route mirror: routed output device kind + calibration-profile identity (AudioDeviceCallback)
    private external fun nativeVolumeDb(db: Float)  // Volume mirror: voice-call stream dB, at start + on VOLUME_CHANGED


    // Held for the service's whole life: Android DROPS WiFi multicast in the radio firmware unless
    // some app holds a MulticastLock — without it the LAN discovery beacons never reach Rust, so a
    // phone sitting NEXT TO its sibling on the same network fell back to the relay ("one device on
    // my network is going thru a relay", 2026-07-26). Battery cost is the multicast filter staying
    // open — the price of same-room discovery actually working.
    private var multicastLock: android.net.wifi.WifiManager.MulticastLock? = null

    override fun onCreate() {
        super.onCreate()
        live = this
        createNotificationChannel()
        val wifi = applicationContext.getSystemService(WIFI_SERVICE) as android.net.wifi.WifiManager
        multicastLock = wifi.createMulticastLock("photon-lan").apply {
            setReferenceCounted(false)
            acquire()
        }
        PhotonLog.d(TAG, "Service created (multicast lock held)")
        registerAudioMirrors()
        reportPriorExits()
    }

    /** Calibration substrate (docs plan 2026-09-02): mirror the routed OUTPUT device + the voice-call volume down to Rust, edge-driven — an AudioDeviceCallback for route changes and the system VOLUME_CHANGED broadcast for the knob. Rust keys calibration profiles on the route identity and scales the echo prediction by the dB delta from calibration time. */
    private fun registerAudioMirrors() {
        val am = getSystemService(AUDIO_SERVICE) as android.media.AudioManager
        fun pushRoute() {
            // The device the call audio actually routes to: prefer BT SCO/A2DP, then wired, then earpiece/speaker — mirroring Android's own routing priority.
            val outs = am.getDevices(android.media.AudioManager.GET_DEVICES_OUTPUTS)
            var kind = -1
            var id = "unknown"
            fun rank(t: Int): Int = when (t) {
                android.media.AudioDeviceInfo.TYPE_BLUETOOTH_SCO, android.media.AudioDeviceInfo.TYPE_BLUETOOTH_A2DP -> 4
                android.media.AudioDeviceInfo.TYPE_WIRED_HEADSET, android.media.AudioDeviceInfo.TYPE_WIRED_HEADPHONES, android.media.AudioDeviceInfo.TYPE_USB_HEADSET -> 3
                android.media.AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> 2
                android.media.AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> 1
                else -> 0
            }
            val dev = outs.maxByOrNull { rank(it.type) }
            if (dev != null) {
                when (rank(dev.type)) {
                    4 -> { kind = 3; id = "bt:${dev.productName}" }
                    3 -> { kind = 2; id = "headset:${dev.productName}" }
                    2 -> { kind = 1; id = "earpiece" }
                    1 -> { kind = 0; id = "speaker" }
                    else -> { kind = -1; id = "unknown:${dev.productName}" }
                }
            }
            try { nativeAudioRoute(kind, id) } catch (e: Throwable) { PhotonLog.w(TAG, "route mirror failed: ${e.message}") }
        }
        fun pushVolume() {
            try {
                val db = if (Build.VERSION.SDK_INT >= 28) {
                    am.getStreamVolumeDb(
                        android.media.AudioManager.STREAM_VOICE_CALL,
                        am.getStreamVolume(android.media.AudioManager.STREAM_VOICE_CALL),
                        android.media.AudioDeviceInfo.TYPE_BUILTIN_SPEAKER,
                    )
                } else {
                    // Pre-28 fallback: linear index ratio → rough dB (20·log10), floor -60.
                    val v = am.getStreamVolume(android.media.AudioManager.STREAM_VOICE_CALL).toFloat()
                    val max = am.getStreamMaxVolume(android.media.AudioManager.STREAM_VOICE_CALL).toFloat().coerceAtLeast(1f)
                    if (v <= 0f) -60f else (20.0 * Math.log10((v / max).toDouble())).toFloat()
                }
                nativeVolumeDb(db)
            } catch (e: Throwable) { PhotonLog.w(TAG, "volume mirror failed: ${e.message}") }
        }
        am.registerAudioDeviceCallback(object : android.media.AudioDeviceCallback() {
            override fun onAudioDevicesAdded(added: Array<out android.media.AudioDeviceInfo>?) { pushRoute() }
            override fun onAudioDevicesRemoved(removed: Array<out android.media.AudioDeviceInfo>?) { pushRoute() }
        }, null)
        registerReceiver(object : android.content.BroadcastReceiver() {
            override fun onReceive(c: android.content.Context?, i: Intent?) { pushVolume() }
        }, android.content.IntentFilter("android.media.VOLUME_CHANGED_ACTION"))
        pushRoute()
        pushVolume()
    }

    /** The system's account of every death the process could not witness itself — ANR, low-memory kill, native crash, explicit stop — logged at the NEXT start so it rides the next submission. The in-process panic hook + crash sidecar cover Rust panics with file and line; this covers everything that bypasses a hook entirely (SIGKILL has no hook), and for ANRs/native crashes attaches the system's own trace. Watermarked in prefs so each exit reports exactly once. API 30+; older devices keep sidecar-only coverage. */
    private fun reportPriorExits() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        try {
            val am = getSystemService(android.app.ActivityManager::class.java) ?: return
            val prefs = getSharedPreferences("photon_exit_watermark", MODE_PRIVATE)
            val watermark = prefs.getLong("last_reported", 0L)
            var newest = watermark
            for (exit in am.getHistoricalProcessExitReasons(packageName, 0, 8)) {
                if (exit.timestamp <= watermark) continue
                if (exit.timestamp > newest) newest = exit.timestamp
                val reason = when (exit.reason) {
                    android.app.ApplicationExitInfo.REASON_ANR -> "ANR"
                    android.app.ApplicationExitInfo.REASON_CRASH -> "CRASH"
                    android.app.ApplicationExitInfo.REASON_CRASH_NATIVE -> "CRASH_NATIVE"
                    android.app.ApplicationExitInfo.REASON_LOW_MEMORY -> "LOW_MEMORY"
                    android.app.ApplicationExitInfo.REASON_SIGNALED -> "SIGNALED(${exit.status})"
                    android.app.ApplicationExitInfo.REASON_EXIT_SELF -> "EXIT_SELF"
                    android.app.ApplicationExitInfo.REASON_USER_REQUESTED -> "USER_REQUESTED"
                    android.app.ApplicationExitInfo.REASON_USER_STOPPED -> "USER_STOPPED"
                    android.app.ApplicationExitInfo.REASON_PACKAGE_UPDATED -> "PACKAGE_UPDATED"
                    android.app.ApplicationExitInfo.REASON_PERMISSION_CHANGE -> "PERMISSION_CHANGE"
                    else -> "OTHER(${exit.reason})"
                }
                PhotonLog.i("ExitInfo", "PRIOR RUN DIED (system): reason=$reason importance=${exit.importance} pss=${exit.pss}KB rss=${exit.rss}KB at=${exit.timestamp} desc=${exit.description ?: ""}")
                // ANR / native-crash traces are the payload that names the stall or the frame — first 4KB is plenty for the log.
                if (exit.reason == android.app.ApplicationExitInfo.REASON_ANR || exit.reason == android.app.ApplicationExitInfo.REASON_CRASH_NATIVE) {
                    try {
                        exit.traceInputStream?.use { ts ->
                            val head = String(ts.readBytes().take(4096).toByteArray())
                            for (line in head.lineSequence().take(60)) PhotonLog.i("ExitInfo", "trace: $line")
                        }
                    } catch (_: Exception) {}
                }
            }
            if (newest != watermark) prefs.edit().putLong("last_reported", newest).apply()
        } catch (e: Exception) {
            PhotonLog.w("ExitInfo", "exit-reason read failed", e)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Answer/Decline pressed ON the call notification (docs/calls.md redesign 2026-08-30): hand the verdict to Rust (the app tick drains it into answer/decline on the UI thread), drop the notification, and — on answer — bring the Activity forward so the call screen is in hand.
        when (intent?.action) {
            ACTION_ANSWER_CALL, ACTION_DECLINE_CALL -> {
                val answer = intent.action == ACTION_ANSWER_CALL
                nativeCallAction(answer)
                cancelCallNotification()
                if (answer) {
                    startActivity(Intent(this, PhotonActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                }
                requestServiceTick()
                return START_STICKY
            }
        }
        // Extract fingerprint and data dirs from intent. shadowDir is "" if getExternalFilesDir returned null on the Activity side — Rust treats empty as "fall back to dataDir with shadow-suffix filename".
        val fingerprint = intent?.getByteArrayExtra("fingerprint")
        val dataDir = intent?.getStringExtra("dataDir")
        val shadowDir = intent?.getStringExtra("shadowDir") ?: ""

        if (fingerprint != null && dataDir != null && networkPtr == 0L) {
            // Per-boot secret for the session capsule's boot-lock (wairua): the app-private session file is FBE-sandbox-protected, so this value only needs to be per-boot-STABLE, not secret. Settings.Global.BOOT_COUNT is exactly that — constant during a boot, incremented on reboot — and readable with no permission on every ROM, unlike /proc/sys/kernel/random/boot_id which locked-down Samsung SELinux blocks from untrusted_app (which silently broke session persistence there: re-attest on every app restart). Prefix-tagged so it can never collide with a raw boot_id string.
            val bootSecret = try {
                val bc = android.provider.Settings.Global.getInt(contentResolver, "boot_count", 0)
                "bootcount:$bc".toByteArray()
            } catch (e: Exception) {
                // No BOOT_COUNT (pre-API-24 or a stripped ROM): fall back to a per-install random persisted in prefs. Stable across restarts AND reboots (so a reboot won't force re-attest), which is a weaker forward-secrecy posture than the boot-lock but strictly better than being logged out constantly.
                val prefs = getSharedPreferences("photon_boot", MODE_PRIVATE)
                var s = prefs.getString("secret", null)
                if (s == null) { s = java.util.UUID.randomUUID().toString(); prefs.edit().putString("secret", s).apply() }
                "installsecret:$s".toByteArray()
            }
            // Initialize network stack
            networkPtr = nativeNetworkInit(fingerprint, dataDir, shadowDir, bootSecret)
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

        // EXPLICIT dataSync-only foreground type at launch. The two-arg startForeground inherits the manifest's FULL type set (dataSync|microphone), and Android 14 (targetSDK 34) hard-rejects a microphone-type FGS start without RECORD_AUDIO granted + the app in an eligible state — which is every fresh launch now that the mic prompt is deferred to the first call (launch crash on Android 14 Samsungs, field 2026-08-19; pre-14 devices don't enforce and were fine). The mic type is ADDED at call time by promoteForeground(true) in startCapture and dropped again at stopCallAudio.
        promoteForeground(false)
        return START_STICKY
    }

    /** (Re)assert the foreground notification with explicit FGS types: dataSync always, microphone only when a call is actively capturing (withMic). API 29- has no typed startForeground; the MICROPHONE constant is API 30+. SecurityException is caught, not fatal: an ineligible mic promotion (e.g., backgrounded edge) degrades to listen-only instead of crashing the whole app. */
    private fun promoteForeground(withMic: Boolean) {
        val notification = buildNotification()
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                var types = ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
                if (withMic && Build.VERSION.SDK_INT >= 30) {
                    types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
                }
                startForeground(NOTIFICATION_ID, notification, types)
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (e: Exception) {
            PhotonLog.w(TAG, "startForeground(withMic=$withMic) rejected — continuing degraded", e)
        }
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        live = null
        PhotonLog.d(TAG, "Service destroying")
        multicastLock?.release()
        multicastLock = null
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
     * Re-post the sticky session broadcast ONLY if the OS has evicted it (Samsung drops stickies
     * aggressively). Reads it first and skips the re-post when it's still present, so the periodic
     * freshness check never churns a broadcast that's already there. Safe when signed out: the
     * native send no-ops when tohu::session() is None.
     */
    fun ensureSessionBroadcast() {
        if (readSessionBroadcast() == null) {
            PhotonLog.d(TAG, "session sticky missing — re-posting")
            sendSessionBroadcast()
        }
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
            // IMPORTANCE_MIN = the legal minimum for a foreground service: NO status-bar icon, silent, collapsed under the shade's quiet section. The OS mandates the notification EXIST (it's the contract that keeps the service scheduled); this is as invisible as it gets. On Android 13+ the user can additionally swipe it away and the service keeps running.
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Connection Status",
                NotificationManager.IMPORTANCE_MIN
            ).apply {
                description = "Quiet indicator that Photon is maintaining a secure connection"
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
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setSilent(true)
            .setShowWhen(false)
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
     * one entry. [sender] / [text] are the real display name and decrypted message BY DESIGN: Rust now
     * fires this post-decrypt (the pre-decrypt worker no longer notifies at all), and hiding content on
     * the lock screen is the OS's job via its per-app sensitive-content setting. The sender key still
     * never crosses — only its rendered audio/haptic and the display name do.
     */
    fun postMessageNotification(wav: ByteArray, timings: LongArray, amplitudes: IntArray, sender: String, text: String) {
        // No foreground bail here: RUST owns the suppression decision (it calls this only when the
        // user is NOT looking at the sender's conversation — see photon_app's `looking` gate). The
        // old blanket inForeground return silenced messages from everyone else while the app was
        // open on any other screen.

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
            .setContentTitle(sender)
            .setContentText(text)
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
    /** Foreground ring (called from Rust): the full-screen in-app ring panel is on screen, so ONLY the relationship ring chirp + haptic play — no notification (a heads-up banner used to cover the old top-bar Answer button). */
    fun playRingAlert(wav: ByteArray, timings: LongArray, amplitudes: IntArray) {
        playChirp(wav)
        vibrateChirp(timings, amplitudes)
    }

    /** Backgrounded/locked ring (called from Rust): the OS's blessed incoming-call surface — CATEGORY_CALL, fullScreenIntent (locked phone launches straight into the in-app ring panel), Answer/Decline actions ON the notification so nothing can cover them, ongoing (only a stop edge cancels it — see cancelCallNotification). */
    fun postCallNotification(wav: ByteArray, timings: LongArray, amplitudes: IntArray, sender: String, text: String) {
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
        val fullScreen = PendingIntent.getActivity(
            this,
            1,
            Intent(this, PhotonActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE
        )
        val answer = PendingIntent.getService(
            this,
            2,
            Intent(this, PhotonConnectionService::class.java).setAction(ACTION_ANSWER_CALL),
            PendingIntent.FLAG_IMMUTABLE
        )
        val decline = PendingIntent.getService(
            this,
            3,
            Intent(this, PhotonConnectionService::class.java).setAction(ACTION_DECLINE_CALL),
            PendingIntent.FLAG_IMMUTABLE
        )
        val notification = NotificationCompat.Builder(this, PhotonActivity.CHANNEL_ID)
            .setContentTitle(sender)
            .setContentText(text)
            .setSmallIcon(android.R.drawable.sym_action_call)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setOngoing(true)
            .setAutoCancel(false)
            .setContentIntent(fullScreen)
            .setFullScreenIntent(fullScreen, true)
            .addAction(0, "Decline", decline)
            .addAction(0, "Answer", answer)
            .build()
        getSystemService(NotificationManager::class.java).notify(CALL_NOTIFICATION_ID, notification)
        playChirp(wav)
        vibrateChirp(timings, amplitudes)
    }

    /** Ring-stop edge (answered anywhere, declined, caller hangup): tear the ongoing call notification down. */
    fun cancelCallNotification() {
        getSystemService(NotificationManager::class.java).cancel(CALL_NOTIFICATION_ID)
    }

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

    // ------------------------------------------------------------------
    // Voice-call audio (docs/calls.md): Kotlin owns the device loops, Rust owns the queues.
    // AudioRecord uses VOICE_COMMUNICATION — that source selection is what engages the vendor
    // acoustic echo canceller (echo layer 1); AudioTrack mirrors it with USAGE_VOICE_COMMUNICATION.
    // 48kHz mono PCM16 in 480-sample (10ms) frames both directions, matching Rust's FRAME_SAMPLES.
    // ------------------------------------------------------------------

    private external fun nativeAudioCaptured(samples: ShortArray)
    private external fun nativeAudioNextFrame(): ShortArray

    @Volatile private var callAudioRunning = false
    private var captureThread: Thread? = null
    private var renderThread: Thread? = null

    /** Start (or, when the mic permission lands mid-call, RE-start) the capture leg. Idempotent: no-op
     *  if no call is live or capture is already running. Missing permission → ask the Activity to
     *  prompt at THIS call (PhotonActivity.requestMicPermission), whose grant callback calls back here
     *  so the prompting call goes hot rather than staying mute until the next one. Public so the
     *  Activity's permission-result callback can invoke it. */
    fun startCapture() {
        if (!callAudioRunning) return
        if (captureThread?.isAlive == true) return
        val sampleRate = 48000
        val frameSamples = 480
        val hasMic = checkSelfPermission(android.Manifest.permission.RECORD_AUDIO) ==
            android.content.pm.PackageManager.PERMISSION_GRANTED
        if (!hasMic) {
            PhotonLog.w(TAG, "callAudio: RECORD_AUDIO not granted — prompting at the call; listen-only until granted")
            PhotonActivity.live?.requestMicPermission()
            return
        }
        // Android 14 FGS discipline: the microphone type is added to the foreground service ONLY now — at an actual call, with the permission granted and the app foreground (the user just tapped answer/call). Launch-time mic-typed startForeground is what crashed Android 14 devices.
        promoteForeground(true)
        captureThread = Thread({
            try {
                val minBuf = android.media.AudioRecord.getMinBufferSize(
                    sampleRate,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT
                ).coerceAtLeast(frameSamples * 4)
                // VOICE_RECOGNITION, not VOICE_COMMUNICATION (Nick 2026-08-20, latency-first): the communication source routes thru the vendor voice pipeline — its NS/AGC/AEC chain AND its slow path (~30-40ms in), the largest remaining latency term now that output rides the fast mixer. VOICE_RECOGNITION is the canonical raw fast-track-eligible source: minimal processing, low latency. The trade, taken knowingly: vendor mic-side echo help is GONE, so speakerphone echo gets more audible until our own subtractive canceller (RENDER_REF is the reference, zero added delay) lands. Do NOT attach AcousticEchoCanceler here as a stopgap — effects kick capture off the fast path, un-doing this exact win.
                val rec = android.media.AudioRecord(
                    android.media.MediaRecorder.AudioSource.VOICE_RECOGNITION,
                    sampleRate,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                    minBuf
                )
                rec.startRecording()
                PhotonLog.i(TAG, "callAudio: capture up (VOICE_RECOGNITION raw fast-path, buf=$minBuf, granted=${rec.bufferSizeInFrames}fr)")
                val buf = ShortArray(frameSamples)
                while (callAudioRunning) {
                    var off = 0
                    while (off < frameSamples && callAudioRunning) {
                        val n = rec.read(buf, off, frameSamples - off)
                        if (n <= 0) break
                        off += n
                    }
                    if (off == frameSamples) nativeAudioCaptured(buf.copyOf())
                }
                rec.stop(); rec.release()
            } catch (e: Exception) {
                PhotonLog.w(TAG, "callAudio capture failed", e)
            }
        }, "call-capture").also { it.start() }
    }

    /** Called from Rust (call_service_void) when a call goes active — for BOTH an outgoing call's
     *  answer and an incoming answer. Starts playback always and mic capture if permitted; a missing
     *  mic permission is prompted HERE (at the call, not at launch) via startCapture, and the grant
     *  re-runs capture so this very call goes hot. */
    fun startCallAudio() {
        if (callAudioRunning) return
        callAudioRunning = true
        val sampleRate = 48000
        val frameSamples = 480

        startCapture()

        renderThread = Thread({
            try {
                // LOW-LATENCY OUTPUT (Nick's call 2026-08-19): VOICE_COMMUNICATION forced the vendor voice pipeline — hardware AEC, but an 80ms buffer floor and no fast mixer (measured: lowLatency=false, granted=80ms), which is the whole of the Nick→Emma delay.
                // Trade the vendor echo canceller for latency: USAGE_MEDIA rides the fast-mixer path and echo is left to our software suppression duck.
                // The buffer is sized to a few NATIVE bursts (PROPERTY_OUTPUT_FRAMES_PER_BUFFER), NOT getMinBufferSize — a small burst-aligned buffer is what actually lets the fast track engage; the 80ms minBuf disqualified it — with a floor of two 10ms write chunks so a write always fits.
                // The fast path also needs our 48k stream to match the device's native output rate, so nativeRate is logged: a mismatch is the usual reason lowLatency comes back false.
                val am = applicationContext.getSystemService(android.content.Context.AUDIO_SERVICE) as android.media.AudioManager
                val nativeBurst = am.getProperty(android.media.AudioManager.PROPERTY_OUTPUT_FRAMES_PER_BUFFER)?.toIntOrNull() ?: frameSamples
                val nativeRate = am.getProperty(android.media.AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)?.toIntOrNull() ?: sampleRate
                val bufFrames = maxOf(nativeBurst * 4, frameSamples * 2)
                val track = AudioTrack.Builder()
                    .setAudioAttributes(
                        AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_MEDIA)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                            .build()
                    )
                    .setAudioFormat(
                        AudioFormat.Builder()
                            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                            .setSampleRate(sampleRate)
                            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                            .build()
                    )
                    .setBufferSizeInBytes(bufFrames * 2)
                    .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                    .setTransferMode(AudioTrack.MODE_STREAM)
                    .build()
                track.play()
                // MEASURED, not estimated: getBufferSizeInFrames is what the track ACTUALLY granted (the framework can clamp a too-small ask up, or fall off the fast path) and getPerformanceMode says whether the fast mixer really engaged.
                val gotFrames = track.bufferSizeInFrames
                val fast = track.performanceMode == AudioTrack.PERFORMANCE_MODE_LOW_LATENCY
                PhotonLog.i(TAG, "callAudio: render up (MEDIA, req=${bufFrames}fr granted=${gotFrames}fr=${gotFrames * 1000 / sampleRate}ms lowLatency=$fast burst=$nativeBurst nativeRate=$nativeRate)")
                while (callAudioRunning) {
                    // Rust hands back 10ms of decoded far-end (silence when the jitter buffer is dry) —
                    // the blocking write paces this loop at the device's real drain rate.
                    val frame = nativeAudioNextFrame()
                    if (frame.isNotEmpty()) track.write(frame, 0, frame.size)
                }
                // Underrun count = how often we starved the device buffer over the whole call: 0 means the shallow buffer + adaptive jitter held, a climbing count says the floor is too low for this path and the jitter buffer should rest deeper.
                PhotonLog.i(TAG, "callAudio: render down (underruns=${track.underrunCount})")
                track.stop(); track.release()
            } catch (e: Exception) {
                PhotonLog.w(TAG, "callAudio render failed", e)
            }
        }, "call-render").also { it.start() }
    }

    /** Called from Rust at hangup — loops observe the flag and tear their devices down. */
    fun stopCallAudio() {
        callAudioRunning = false
        captureThread = null
        renderThread = null
        // Drop the microphone FGS type the moment the call ends — back to dataSync-only (privacy indicator off, Android 14 mic-FGS accounting closed).
        promoteForeground(false)
        PhotonLog.i(TAG, "callAudio: stopped")
    }
}
