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
        bringBack(context)
    }

    companion object {
        /** After a self-update the old process is gone and whatever the system installer routed thru (the "install unknown apps" settings page, the confirm dialog) is left on top of our task — the user reads that as a hang (Nick's Note 10, 2026-09-10, twice). Try to relaunch the activity outright (Android may refuse a background start), and ALWAYS post a tap-to-open notification so there is one obvious way back in. */
        fun bringBack(context: Context) {
            val launch = Intent(context, PhotonActivity::class.java).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            }
            try {
                context.startActivity(launch)
                PhotonLog.i("Update", "relaunched photon after the update")
            } catch (e: Exception) {
                PhotonLog.i("Update", "relaunch refused (${e.message}) — the notification carries it")
            }
            try {
                val nm = context.getSystemService(android.app.NotificationManager::class.java)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && nm.getNotificationChannel(PhotonActivity.CHANNEL_ID) == null) {
                    nm.createNotificationChannel(android.app.NotificationChannel(PhotonActivity.CHANNEL_ID, PhotonActivity.CHANNEL_NAME, android.app.NotificationManager.IMPORTANCE_HIGH))
                }
                val pi = android.app.PendingIntent.getActivity(
                    context, 7, launch,
                    if (Build.VERSION.SDK_INT >= 31) android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_IMMUTABLE else android.app.PendingIntent.FLAG_UPDATE_CURRENT
                )
                val version = try { context.packageManager.getPackageInfo(context.packageName, 0).versionName } catch (e: Exception) { "" }
                val n = androidx.core.app.NotificationCompat.Builder(context, PhotonActivity.CHANNEL_ID)
                    .setSmallIcon(R.mipmap.ic_launcher)
                    .setContentTitle("Photon updated" + if (version.isNullOrEmpty()) "" else " to $version")
                    .setContentText("Tap to open")
                    .setContentIntent(pi)
                    .setAutoCancel(true)
                    .setPriority(androidx.core.app.NotificationCompat.PRIORITY_HIGH)
                    .build()
                nm.notify(7001, n)
            } catch (e: Exception) {
                PhotonLog.e("Update", "post-update notification failed: ${e.message}")
            }
        }
    }
}
