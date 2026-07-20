package com.photon.messenger

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

/**
 * Firebase Cloud Messaging receiver — two message kinds:
 *
 * `wake`: the reachability doorbell (docs/reachability-doorbell.md). An EMPTY high-priority data
 * push a peer's worker relays when direct delivery keeps missing and we've been silent past the
 * dozed threshold. It carries nothing and names nobody — it is a remote wake() syscall. Warm
 * process → poke the connection service into a protocol tick (pings go out, the NAT hole
 * re-punches, direct delivery resumes and the real message arrives on the free path). Cold
 * process (rebooted phone, never opened) → post the generic "New message" notification; opening
 * the app attests and pulls. Either way Google saw only THAT a wake happened.
 *
 * `peer_update`: legacy topic-broadcast cache invalidation (slated for gossip-subsumption).
 */
class PhotonMessagingService : FirebaseMessagingService() {

    companion object {
        init {
            System.loadLibrary("photon_messenger")
        }
        private const val TAG = "PhotonFCM"
    }

    // Native method to notify Rust of peer update
    private external fun nativePeerUpdateReceived()
    private external fun nativeSetFcmToken(token: String, projectId: String)
    private external fun nativeUpdateNoticeReceived()

    override fun onMessageReceived(remoteMessage: RemoteMessage) {
        super.onMessageReceived(remoteMessage)

        when (remoteMessage.data["type"]) {
            "wake" -> {
                val svc = PhotonConnectionService.live
                if (svc != null) {
                    PhotonLog.d(TAG, "Doorbell wake — poking protocol tick")
                    svc.requestServiceTick()
                } else {
                    PhotonLog.d(TAG, "Doorbell wake with no live service — posting notification")
                    postWakeNotification()
                }
            }
            "update" -> {
                // Release notice off the `updates` topic — a deploy shipped. Flag the manifest poll due; the check runs on the next UI tick (a dozed phone learns on next open — updates aren't message-urgent, so no wakelock ceremony here). Advisory only: what installs is still gated by the manifest signature + stamp window.
                PhotonLog.d(TAG, "Release notice — flagging update check")
                nativeUpdateNoticeReceived()
            }
            "peer_update" -> {
                PhotonLog.d(TAG, "Peer update received from FGTW")
                // Poke Rust - it will trigger a full peer list refresh
                nativePeerUpdateReceived()
            }
        }
    }

    override fun onNewToken(token: String) {
        super.onNewToken(token)
        PhotonLog.d(TAG, "FCM token refreshed")
        // A rotated token invalidates the published bell — hand the fresh one to Rust; the ping
        // cycle notices the change and re-publishes to the worker's bell registry.
        try {
            val projectId = com.google.firebase.FirebaseApp.getInstance().options.projectId ?: ""
            if (projectId.isNotEmpty()) nativeSetFcmToken(token, projectId)
        } catch (e: Exception) {
            PhotonLog.w(TAG, "FCM token handoff failed", e)
        }
    }

    /**
     * Cold-wake fallback: the process was started by FCM itself, so there is no session, no keys,
     * and nothing to decrypt — the honest maximum is the same generic banner the warm path shows.
     * Channel creation is idempotent and MUST match the Activity's silent definition (whoever
     * creates it first wins and its sound settings become immutable).
     */
    private fun postWakeNotification() {
        if (PhotonActivity.inForeground) return
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
            .build()
        getSystemService(NotificationManager::class.java)
            .notify(PhotonConnectionService.MESSAGE_NOTIFICATION_ID, notification)
    }
}
