package com.photon.messenger

import android.content.Context

/** The self-update install session (docs/updates.md, Android path), shared by the screen and the background service.
 *  PackageInstaller SESSION: on Android 12+ with photon as its own installer of record the install is UNATTENDED (USER_ACTION_NOT_REQUIRED); otherwise PhotonInstallReceiver gets STATUS_PENDING_USER_ACTION and shows the one-time confirm.
 *  The service calls this while no screen shows and no wave is live (Nick 2026-11-04: an update must land when it posts, not five seconds into the next wave), the screen only for an update the human pressed for. */
object PhotonSelfUpdate {
    /** Where the last staged APK lives, kept so a refused install can be offered again by hand (PhotonInstallReceiver) even after this process is gone. App-private preferences; a path, nothing secret. */
    private const val PREFS = "photon_update"
    private const val KEY_STAGED = "staged_apk"

    fun stagedApk(context: Context): String? =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_STAGED, null)?.takeIf { java.io.File(it).isFile }

    /** PLAY PROTECT REFUSED IT (field 2026-11-05: Theresa sat on v126 for weeks, every background install rejected with INSTALL_FAILED_VERIFICATION_FAILURE and nothing on her screen said so).
     *  A notification that survives the app being closed: tapping it opens Android's own installer on the APK already downloaded, where Play Protect's dialog lets the human choose to install anyway; with no APK left it opens the download page instead. */
    fun notifyBlocked(context: Context, reason: String) {
        try {
            val nm = context.getSystemService(android.app.NotificationManager::class.java)
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O && nm.getNotificationChannel(PhotonActivity.CHANNEL_ID) == null) {
                nm.createNotificationChannel(android.app.NotificationChannel(PhotonActivity.CHANNEL_ID, PhotonActivity.CHANNEL_NAME, android.app.NotificationManager.IMPORTANCE_HIGH))
            }
            val staged = stagedApk(context)
            val open = if (staged != null) {
                val uri = androidx.core.content.FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", java.io.File(staged))
                android.content.Intent(android.content.Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, "application/vnd.android.package-archive")
                    addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                }
            } else {
                android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse("https://passless.org/photon/")).apply {
                    addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                }
            }
            val flags = if (android.os.Build.VERSION.SDK_INT >= 31) android.app.PendingIntent.FLAG_UPDATE_CURRENT or android.app.PendingIntent.FLAG_IMMUTABLE else android.app.PendingIntent.FLAG_UPDATE_CURRENT
            val pi = android.app.PendingIntent.getActivity(context, 8, open, flags)
            val n = androidx.core.app.NotificationCompat.Builder(context, PhotonActivity.CHANNEL_ID)
                .setSmallIcon(R.mipmap.ic_launcher)
                .setContentTitle("Photon could not update itself")
                .setContentText("Your phone blocked the update. Tap to install it yourself.")
                .setStyle(androidx.core.app.NotificationCompat.BigTextStyle().bigText(
                    "Your phone's app scanner blocked Photon's automatic update, so you are still on an older version that may not reach your friends. " +
                        "Tap here, then if a warning appears, tap its small text for more details and choose Install anyway."))
                .setContentIntent(pi)
                .setAutoCancel(true)
                .setPriority(androidx.core.app.NotificationCompat.PRIORITY_HIGH)
                .build()
            nm.notify(7002, n)
            PhotonLog.i("Update", "install blocked ($reason) — the human was told, with the APK ${if (staged != null) "ready to install by hand" else "gone; the download page offered"}")
        } catch (e: Exception) {
            PhotonLog.e("Update", "blocked-install notification failed: ${e.message}")
        }
    }

    fun installSession(context: Context, path: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY_STAGED, path).apply()
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
