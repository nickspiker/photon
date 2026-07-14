package com.photon.messenger

import android.Manifest
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.bluetooth.le.BluetoothLeScanner
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat

/**
 * Pairing v2 proximity beacon (docs/pairing-v2.md) — the Android courier.
 * Rust drives it thru the JNI bridge (startAdvertise/stopAdvertise/startScan/stopScan called
 * on this object via the global ref cached in nativeInit); heard frames go back down via
 * nativeOnBeaconHeard. The frame is split across two manufacturer-data ids (0xFFFF = ADV
 * chunk, 0xFFFE = scan-response chunk) because ScanRecord merges ADV+SCAN_RSP into one map
 * keyed by company id — reassembly is a concat here, so Rust always sees one whole frame.
 * BLUETOOTH_SCAN/ADVERTISE are runtime permissions on Android 12+: a start call without the
 * grant stashes itself as pending, fires the Activity's request dialog, and re-runs on grant.
 */
object PhotonBeacon {
    private var appContext: Context? = null
    private var activity: PhotonActivity? = null
    private var advertiser: BluetoothLeAdvertiser? = null
    private var advertiseCallback: AdvertiseCallback? = null
    private var scanner: BluetoothLeScanner? = null
    private var scanCallback: ScanCallback? = null
    private var pendingAdvertise: Pair<ByteArray, ByteArray>? = null
    private var pendingScan = false

    private external fun nativeInit()
    private external fun nativeOnBeaconHeard(frame: ByteArray)

    /** Called once from PhotonActivity.onCreate (after loadLibrary): caches contexts and registers the JNI bridge. */
    fun init(a: PhotonActivity) {
        appContext = a.applicationContext
        activity = a
        nativeInit()
    }

    /** Permission dialog came back positive: re-run whatever start call was waiting on it. */
    fun onPermissionsGranted() {
        pendingAdvertise?.let { startAdvertise(it.first, it.second) }
        if (pendingScan) startScan()
    }

    private fun hasPerm(p: String): Boolean {
        val ctx = appContext ?: return false
        return ContextCompat.checkSelfPermission(ctx, p) == PackageManager.PERMISSION_GRANTED
    }

    private fun adapter() = (appContext?.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter

    /** Advertise the beacon frame: adv chunk under 0xFFFF, scan-response chunk under 0xFFFE. Non-connectable, low latency, no timeout — Rust's guard decides when it stops. */
    fun startAdvertise(advChunk: ByteArray, rspChunk: ByteArray) {
        if (Build.VERSION.SDK_INT >= 31 && !hasPerm(Manifest.permission.BLUETOOTH_ADVERTISE)) {
            pendingAdvertise = Pair(advChunk, rspChunk)
            PhotonLog.i("Beacon", "advertise waiting on permission")
            activity?.requestBlePermissions()
            return
        }
        val adapter = adapter() ?: run { PhotonLog.w("Beacon", "no bluetooth adapter"); return }
        if (!adapter.isEnabled) { PhotonLog.w("Beacon", "bluetooth is off — cannot advertise"); return }
        val adv = adapter.bluetoothLeAdvertiser ?: run { PhotonLog.w("Beacon", "chipset cannot advertise"); return }
        stopAdvertise()
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .setConnectable(false)
            .setTimeout(0)
            .build()
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addManufacturerData(0xFFFF, advChunk)
            .build()
        val scanRsp = if (rspChunk.isNotEmpty()) {
            AdvertiseData.Builder()
                .setIncludeDeviceName(false)
                .addManufacturerData(0xFFFE, rspChunk)
                .build()
        } else null
        val cb = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
                PhotonLog.i("Beacon", "advertising (${advChunk.size}+${rspChunk.size} bytes)")
            }
            override fun onStartFailure(errorCode: Int) {
                PhotonLog.e("Beacon", "advertise failed, code $errorCode")
            }
        }
        try {
            adv.startAdvertising(settings, data, scanRsp, cb)
            advertiser = adv
            advertiseCallback = cb
            pendingAdvertise = null
        } catch (e: SecurityException) {
            PhotonLog.e("Beacon", "advertise SecurityException: ${e.message}")
        }
    }

    fun stopAdvertise() {
        pendingAdvertise = null
        val adv = advertiser ?: return
        val cb = advertiseCallback ?: return
        try { adv.stopAdvertising(cb) } catch (_: SecurityException) {}
        advertiser = null
        advertiseCallback = null
    }

    /** Scan for pairing beacons: filter on manufacturer id 0xFFFF (any payload), reassemble the two chunks, hand the frame to Rust. Rust does the hp-prefix filter and dedup. */
    fun startScan() {
        if (Build.VERSION.SDK_INT >= 31 && !hasPerm(Manifest.permission.BLUETOOTH_SCAN)) {
            pendingScan = true
            PhotonLog.i("Beacon", "scan waiting on permission")
            activity?.requestBlePermissions()
            return
        }
        val adapter = adapter() ?: run { PhotonLog.w("Beacon", "no bluetooth adapter"); return }
        if (!adapter.isEnabled) { PhotonLog.w("Beacon", "bluetooth is off — cannot scan"); return }
        val sc = adapter.bluetoothLeScanner ?: run { PhotonLog.w("Beacon", "no LE scanner"); return }
        stopScan()
        val filters = listOf(
            ScanFilter.Builder().setManufacturerData(0xFFFF, byteArrayOf()).build()
        )
        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
            .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
            .build()
        val cb = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, result: ScanResult) {
                val rec = result.scanRecord ?: return
                val a = rec.getManufacturerSpecificData(0xFFFF) ?: return
                val b = rec.getManufacturerSpecificData(0xFFFE)
                nativeOnBeaconHeard(if (b != null) a + b else a)
            }
            override fun onScanFailed(errorCode: Int) {
                PhotonLog.e("Beacon", "scan failed, code $errorCode")
            }
        }
        try {
            sc.startScan(filters, settings, cb)
            scanner = sc
            scanCallback = cb
            pendingScan = false
            PhotonLog.i("Beacon", "scanning")
        } catch (e: SecurityException) {
            PhotonLog.e("Beacon", "scan SecurityException: ${e.message}")
        }
    }

    fun stopScan() {
        pendingScan = false
        val sc = scanner ?: return
        val cb = scanCallback ?: return
        try { sc.stopScan(cb) } catch (_: SecurityException) {}
        scanner = null
        scanCallback = null
    }
}
