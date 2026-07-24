package com.photon.messenger

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller

/** Result sink for the self-update PackageInstaller session (PhotonActivity.installApkSession). STATUS_PENDING_USER_ACTION carries the system confirm dialog — the one-time bootstrap that makes photon its own installer-of-record (and any OEM that refuses unattended installs); SUCCESS usually arrives after the OS has already swapped the package and restarted us, so it's log-only. */
class PhotonInstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                PhotonLog.i("Update", "install needs the one-time user confirm (installer-of-record bootstrap)")
                @Suppress("DEPRECATION")
                val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
                if (confirm != null) {
                    confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    context.startActivity(confirm)
                }
            }
            PackageInstaller.STATUS_SUCCESS -> PhotonLog.i("Update", "self-update installed")
            else -> PhotonLog.e("Update", "install failed: status=$status ${intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: ""}")
        }
    }
}
