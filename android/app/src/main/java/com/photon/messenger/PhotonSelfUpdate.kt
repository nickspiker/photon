package com.photon.messenger

import android.content.Context

/** The self-update install session (docs/updates.md, Android path), shared by the screen and the background service.
 *  PackageInstaller SESSION: on Android 12+ with photon as its own installer of record the install is UNATTENDED (USER_ACTION_NOT_REQUIRED); otherwise PhotonInstallReceiver gets STATUS_PENDING_USER_ACTION and shows the one-time confirm.
 *  The service calls this while no screen shows and no wave is live (Nick 2026-11-04: an update must land when it posts, not five seconds into the next wave), the screen only for an update the human pressed for. */
object PhotonSelfUpdate {
    fun installSession(context: Context, path: String) {
        val installer = context.packageManager.packageInstaller
        val params = android.content.pm.PackageInstaller.SessionParams(
            android.content.pm.PackageInstaller.SessionParams.MODE_FULL_INSTALL
        ).apply {
            setAppPackageName(context.packageName)
            if (android.os.Build.VERSION.SDK_INT >= 31) {
                setRequireUserAction(android.content.pm.PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
            }
        }
        val sessionId = installer.createSession(params)
        installer.openSession(sessionId).use { session ->
            java.io.File(path).inputStream().use { input ->
                session.openWrite("photon-update.apk", 0, -1).use { out ->
                    input.copyTo(out)
                    session.fsync(out)
                }
            }
            // MUTABLE on purpose: the installer fills EXTRA_INTENT/EXTRA_STATUS into this PendingIntent.
            val intent = android.content.Intent(context, PhotonInstallReceiver::class.java).apply {
                action = "com.photon.messenger.INSTALL_RESULT"
            }
            val flags = if (android.os.Build.VERSION.SDK_INT >= 31) {
                android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_MUTABLE
            } else {
                android.app.PendingIntent.FLAG_UPDATE_CURRENT
            }
            val pi = android.app.PendingIntent.getBroadcast(context, sessionId, intent, flags)
            PhotonLog.i("Update", "committing install session $sessionId (unattended where permitted)")
            session.commit(pi.intentSender)
        }
    }
}
