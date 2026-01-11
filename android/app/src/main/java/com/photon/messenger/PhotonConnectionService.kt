package com.photon.messenger

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
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
        private const val TAG = "PhotonService"
        private const val POLL_INTERVAL_MS = 1000L // 1 second network polling
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
    private external fun nativeNetworkInit(fingerprint: ByteArray, dataDir: String): Long
    private external fun nativeNetworkDestroy(networkPtr: Long)
    private external fun nativeNetworkPoll(networkPtr: Long)  // Check for incoming messages, refresh peers
    private external fun nativeGetDevicePubkey(networkPtr: Long): String

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        Log.d(TAG, "Service created")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Extract fingerprint and data dir from intent
        val fingerprint = intent?.getByteArrayExtra("fingerprint")
        val dataDir = intent?.getStringExtra("dataDir")

        if (fingerprint != null && dataDir != null && networkPtr == 0L) {
            // Initialize network stack
            networkPtr = nativeNetworkInit(fingerprint, dataDir)
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
}
