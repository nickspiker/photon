package com.photon.messenger

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import android.util.Log

/**
 * Firebase Cloud Messaging service for receiving peer update notifications.
 * When FGTW detects a peer IP change, it sends an FCM data message to all devices.
 * This service receives it and pokes the Rust side to trigger a peer list refresh.
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

    override fun onMessageReceived(remoteMessage: RemoteMessage) {
        super.onMessageReceived(remoteMessage)

        // Check if this is a peer_update message
        val messageType = remoteMessage.data["type"]
        if (messageType == "peer_update") {
            Log.d(TAG, "Peer update received from FGTW")
            // Poke Rust - it will trigger a full peer list refresh
            nativePeerUpdateReceived()
        }
    }

    override fun onNewToken(token: String) {
        super.onNewToken(token)
        Log.d(TAG, "FCM token refreshed")
        // Token is sent to FGTW during /announce - no action needed here
        // The token will be included in the next announce when app is active
    }
}
