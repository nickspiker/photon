package com.photon.messenger

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build

/** Restart the connection service the moment OUR package is replaced. An updated app lands in the stopped state and runs NOTHING until something targets it — before this receiver the only thing that ever did was a high-priority FCM doorbell, so an update left photon dead until a friend happened to ring (field, 2026-08-07). MY_PACKAGE_REPLACED is a protected broadcast (only the system sends it) and sits on the background-FGS-start exemption list, and an update stays within one boot, so the boot-locked capsule still opens: the service comes back fully attested with no user action. Reboot deliberately stays cold — the capsule dies with the boot by design; auto-wake after reboot is the fleet-assisted attest work (TICKETS.md). */
class PhotonUpdateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        PhotonLog.i("Update", "package replaced — restarting connection service")
        val fp = DeviceFingerprint.gather(context)
        val serviceIntent = Intent(context, PhotonConnectionService::class.java).apply {
            putExtra("fingerprint", fp.fingerprint)
            putExtra("dataDir", context.filesDir.absolutePath)
            putExtra("shadowDir", context.getExternalFilesDir(null)?.absolutePath ?: "")
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(serviceIntent)
        } else {
            context.startService(serviceIntent)
        }
    }
}
