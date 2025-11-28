package com.photon.messenger

import android.content.Context
import android.os.Build
import android.provider.Settings
import java.io.File

/**
 * Device fingerprint for deterministic key derivation.
 *
 * Combines hardware identifiers to create a stable device fingerprint
 * that survives app reinstalls but changes on factory reset or hardware swap.
 *
 * The fingerprint is NEVER stored - it's derived fresh each time and used
 * immediately to derive the device's Ed25519 keypair via BLAKE3.
 *
 * Note: IMEI/Serial are unavailable on Android 10+ for non-system apps.
 * We use ANDROID_ID + hardware identifiers instead.
 */
object DeviceFingerprint {

    data class FingerprintResult(
        val fingerprint: ByteArray,  // Raw bytes to pass to Rust for BLAKE3 hashing
        val warnings: List<String>   // Any security warnings to show user
    )

    /**
     * Gather device fingerprint components.
     */
    fun gather(context: Context): FingerprintResult {
        val components = mutableListOf<String>()
        val warnings = mutableListOf<String>()

        // 1. ANDROID_ID - persistent per-app, scoped to signing key
        val androidId = Settings.Secure.getString(
            context.contentResolver,
            Settings.Secure.ANDROID_ID
        ) ?: ""
        components.add("android_id:$androidId")

        // 2. Build identifiers - device model fingerprint
        components.add("manufacturer:${Build.MANUFACTURER}")
        components.add("model:${Build.MODEL}")
        components.add("device:${Build.DEVICE}")
        components.add("board:${Build.BOARD}")
        components.add("hardware:${Build.HARDWARE}")

        // 3. CPU info
        val cpuInfo = getCpuInfo()
        components.add("cpu:$cpuInfo")

        // 4. Version tag for future-proofing
        components.add("photon-device-v0")

        // Combine all components into a single string, then to bytes
        val combined = components.joinToString("|")

        return FingerprintResult(
            fingerprint = combined.toByteArray(Charsets.UTF_8),
            warnings = warnings
        )
    }

    /**
     * Get CPU info from /proc/cpuinfo
     */
    private fun getCpuInfo(): String {
        return try {
            val cpuInfo = File("/proc/cpuinfo").readText()
            // Extract key identifiers: Hardware, CPU implementer, CPU part
            val hardware = Regex("Hardware\\s*:\\s*(.+)").find(cpuInfo)?.groupValues?.get(1)?.trim() ?: ""
            val implementer = Regex("CPU implementer\\s*:\\s*(.+)").find(cpuInfo)?.groupValues?.get(1)?.trim() ?: ""
            val part = Regex("CPU part\\s*:\\s*(.+)").find(cpuInfo)?.groupValues?.get(1)?.trim() ?: ""
            "$hardware|$implementer|$part"
        } catch (e: Exception) {
            Build.SUPPORTED_ABIS.joinToString(",")
        }
    }

}
