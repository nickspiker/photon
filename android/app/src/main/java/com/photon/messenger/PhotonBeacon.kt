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
import android.os.ParcelUuid
import androidx.core.content.ContextCompat
import java.nio.ByteBuffer
import java.util.UUID

/**
 * Pairing v2 proximity beacon (docs/pairing-v2.md) — the Android courier.
 * Rust drives it thru the JNI bridge (startAdvertise/stopAdvertise/startScan/stopScan called
 * on this object via the global ref cached in nativeInit); heard beacons go back down via
 * nativeOnBeaconHeard. The beacon is a single 128-bit BLE **service UUID** = `[ magic:4 ][ nonce:4 ][ tag:8 ]`
 * (fgtw::pair::beacon_uuid) — one carrier that works on every platform including macOS, where a
 * service UUID is the only advertising payload Apple allows. Bytes map big-endian: byte 0 is the
 * UUID's most-significant byte, matching Rust's uuid::from_bytes / as_bytes, so the 16 bytes round-trip
 * identically across the wire. BLUETOOTH_SCAN/ADVERTISE are runtime permissions on Android 12+: a start
 * call without the grant stashes itself as pending, fires the Activity's request dialog, and re-runs on grant.
 */
object PhotonBeacon {
    private var appContext: Context? = null
    private var activity: PhotonActivity? = null
    private var advertiser: BluetoothLeAdvertiser? = null
    private var advertiseCallback: AdvertiseCallback? = null
    private var scanner: BluetoothLeScanner? = null
    private var scanCallback: ScanCallback? = null
    private var pendingAdvertise: ByteArray? = null
    private var pendingScan = false

    // The fixed photon-pairing namespace tag (fgtw::pair::BEACON_MAGIC) — the first 4 bytes of every beacon UUID.
    private val MAGIC = byteArrayOf(0xF0.toByte(), 0x70, 0x0B, 0xEA.toByte())

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
        pendingAdvertise?.let { startAdvertise(it) }
        if (pendingScan) startScan()
    }

    private fun hasPerm(p: String): Boolean {
        val ctx = appContext ?: return false
        return ContextCompat.checkSelfPermission(ctx, p) == PackageManager.PERMISSION_GRANTED
    }

    private fun adapter() = (appContext?.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter

    /** 16 bytes → UUID, big-endian (byte 0 = most-significant), inverse of uuidToBytes. */
    private fun bytesToUuid(b: ByteArray): UUID {
        val bb = ByteBuffer.wrap(b)
        return UUID(bb.long, bb.long)
    }

    /** UUID → 16 bytes, big-endian, inverse of bytesToUuid. */
    private fun uuidToBytes(u: UUID): ByteArray =
        ByteBuffer.allocate(16).putLong(u.mostSignificantBits).putLong(u.leastSignificantBits).array()

    /** Advertise the beacon as one 128-bit service UUID (16 bytes + header fits legacy ADV). Non-connectable, low latency, no timeout — Rust's guard decides when it stops. */
    fun startAdvertise(uuid: ByteArray) {
        if (Build.VERSION.SDK_INT >= 31 && !hasPerm(Manifest.permission.BLUETOOTH_ADVERTISE)) {
            pendingAdvertise = uuid
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
            .addServiceUuid(ParcelUuid(bytesToUuid(uuid)))
            .build()
        val cb = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
                PhotonLog.i("Beacon", "advertising ${uuid.size}-byte service uuid")
            }
            override fun onStartFailure(errorCode: Int) {
                PhotonLog.e("Beacon", "advertise failed, code $errorCode")
            }
        }
        try {
            adv.startAdvertising(settings, data, cb)
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

    /** Scan for pairing beacons: hardware-filter on advertised service UUIDs whose first 4 bytes are the photon magic (a UUID + mask), hand each 16-byte UUID to Rust, which magic-checks + resolves it to a fleet device by keyed tag. */
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
        val magicUuid = bytesToUuid(MAGIC + ByteArray(12))
        val maskUuid = bytesToUuid(byteArrayOf(-1, -1, -1, -1) + ByteArray(12))
        val filters = listOf(
            ScanFilter.Builder().setServiceUuid(ParcelUuid(magicUuid), ParcelUuid(maskUuid)).build()
        )
        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
            .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
            .build()
        val cb = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, result: ScanResult) {
                val uuids = result.scanRecord?.serviceUuids ?: return
                for (pu in uuids) {
                    nativeOnBeaconHeard(uuidToBytes(pu.uuid))
                }
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
