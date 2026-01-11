package com.photon.messenger

import android.content.Context
import android.os.Process
import android.provider.Settings

/**
 * Device fingerprint for deterministic key derivation.
 *
 * Uses ANDROID_ID + user number to create a stable device fingerprint.
 *
 * ANDROID_ID is:
 * - Per-device (unique to this physical device)
 * - Per-signing-key (all apps signed with same key get same value)
 * - Persistent across app reinstalls
 * - Reset on factory reset
 *
 * This effectively makes ANDROID_ID a hardware oracle scoped to our signing key.
 * Other developers can't get the same value even on the same device.
 *
 * User number distinguishes between Android multi-user profiles on the same device.
 */
object DeviceFingerprint {

    data class FingerprintResult(
        val fingerprint: ByteArray,  // Raw bytes to pass to Rust for BLAKE3 hashing
        val warnings: List<String>   // Any security warnings to show user
    )

    /**
     * Gather device fingerprint: ANDROID_ID + user number
     */
    fun gather(context: Context): FingerprintResult {
        val warnings = mutableListOf<String>()

        // ANDROID_ID - per-device, per-signing-key oracle
        val androidId = Settings.Secure.getString(
            context.contentResolver,
            Settings.Secure.ANDROID_ID
        ) ?: ""

        // User number - distinguishes multi-user profiles
        val userId = Process.myUserHandle().hashCode()

        // Combine: "android_id:abc123|user:0|photon-v1"
        val fingerprint = "android_id:$androidId|user:$userId|photon-v1"

        return FingerprintResult(
            fingerprint = fingerprint.toByteArray(Charsets.UTF_8),
            warnings = warnings
        )
    }
}
