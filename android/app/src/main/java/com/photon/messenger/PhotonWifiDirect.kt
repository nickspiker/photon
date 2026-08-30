package com.photon.messenger

import android.Manifest
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.net.wifi.p2p.WifiP2pConfig
import android.net.wifi.p2p.WifiP2pManager
import android.net.wifi.p2p.nsd.WifiP2pDnsSdServiceInfo
import android.net.wifi.p2p.nsd.WifiP2pDnsSdServiceRequest
import android.os.Build
import android.util.Base64
import androidx.core.content.ContextCompat

/**
 * Wi-Fi Direct bearer courier (docs/offgrid.md) — the Android radio bridge, same lifecycle
 * pattern as PhotonBeacon: Rust drives it thru the JNI global ref (startAdvertise/startDiscovery/
 * stopAll/createGroup/connectGroup/removeGroup), platform events go back down via
 * nativeOnServiceFound (DNS-SD TXT token blob) and nativeOnGroupChanged (formation state + addresses).
 *
 * Design invariants:
 *  - NEVER touches the infrastructure WiFi connection — STA+P2P concurrency is the whole point.
 *  - Dialog-free: group formation uses pre-shared WifiP2pConfig (networkName+passphrase, API 29+),
 *    which skips the WPS consent dialog on both sides.
 *  - The TXT record carries only rotating keyed tokens (opaque 16-byte chunks) — no identity.
 *  - NEARBY_WIFI_DEVICES (API 33+) / ACCESS_FINE_LOCATION (older) are runtime permissions:
 *    a start call without the grant stashes itself pending and re-runs on grant, like the BLE beacon.
 */
object PhotonWifiDirect {
    private var appContext: Context? = null
    private var activity: PhotonActivity? = null
    private var manager: WifiP2pManager? = null
    private var channel: WifiP2pManager.Channel? = null
    private var receiverRegistered = false
    private var pendingAdvertise: ByteArray? = null
    private var pendingDiscovery = false

    private external fun nativeInit()
    private external fun nativeOnServiceFound(txt: ByteArray)
    private external fun nativeOnGroupChanged(formed: Boolean, isGo: Boolean, ourIp: String, goIp: String)

    /** Called once from PhotonActivity.onCreate (after loadLibrary). */
    fun init(a: PhotonActivity) {
        appContext = a.applicationContext
        activity = a
        nativeInit()
    }

    fun onPermissionsGranted() {
        pendingAdvertise?.let { startAdvertise(it) }
        if (pendingDiscovery) startDiscovery()
    }

    private fun hasPerm(): Boolean {
        val ctx = appContext ?: return false
        val p = if (Build.VERSION.SDK_INT >= 33) Manifest.permission.NEARBY_WIFI_DEVICES
                else Manifest.permission.ACCESS_FINE_LOCATION
        return ContextCompat.checkSelfPermission(ctx, p) == PackageManager.PERMISSION_GRANTED
    }

    private fun mgr(): WifiP2pManager? {
        if (manager == null) {
            val ctx = appContext ?: return null
            manager = ctx.getSystemService(Context.WIFI_P2P_SERVICE) as? WifiP2pManager
            channel = manager?.initialize(ctx, ctx.mainLooper, null)
            if (manager != null && channel != null && !receiverRegistered) {
                val filter = IntentFilter(WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION)
                ctx.registerReceiver(connectionReceiver, filter)
                receiverRegistered = true
            }
        }
        return manager
    }

    /** Group formation edges → Rust. Our own address on the p2p interface comes from the interface itself (the group's iface name), the GO's from connection info. */
    private val connectionReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION) return
            val m = manager ?: return
            val ch = channel ?: return
            try {
                m.requestConnectionInfo(ch) { info ->
                    if (info != null && info.groupFormed) {
                        val goIp = info.groupOwnerAddress?.hostAddress ?: "0.0.0.0"
                        m.requestGroupInfo(ch) { group ->
                            val ourIp = if (info.isGroupOwner) goIp else ifaceV4(group?.getInterface()) ?: "0.0.0.0"
                            PhotonLog.i("WFD", "group up (go=${info.isGroupOwner}) our=$ourIp go=$goIp")
                            nativeOnGroupChanged(true, info.isGroupOwner, ourIp, goIp)
                        }
                    } else {
                        PhotonLog.i("WFD", "group down")
                        nativeOnGroupChanged(false, false, "0.0.0.0", "0.0.0.0")
                    }
                }
            } catch (e: SecurityException) {
                PhotonLog.e("WFD", "connection info SecurityException: ${e.message}")
            }
        }
    }

    /** First IPv4 on the named interface (the client's DHCP address inside the group). */
    private fun ifaceV4(name: String?): String? {
        if (name.isNullOrEmpty()) return null
        return try {
            java.net.NetworkInterface.getByName(name)?.inetAddresses?.toList()
                ?.filterIsInstance<java.net.Inet4Address>()?.firstOrNull()?.hostAddress
        } catch (_: Exception) { null }
    }

    /** Advertise our rotating friend tokens as a `_photon._udp` DNS-SD local service. The blob is base64-chunked across TXT keys (a single TXT value caps at 255 bytes). Instance name is random per call — never an identifier. */
    fun startAdvertise(txtTokens: ByteArray) {
        if (!hasPerm()) {
            pendingAdvertise = txtTokens
            PhotonLog.i("WFD", "advertise waiting on permission")
            activity?.requestWfdPermissions()
            return
        }
        val m = mgr() ?: run { PhotonLog.w("WFD", "no p2p manager"); return }
        val ch = channel ?: return
        val b64 = Base64.encodeToString(txtTokens, Base64.NO_WRAP or Base64.NO_PADDING)
        val txt = HashMap<String, String>()
        var i = 0
        var k = 0
        while (i < b64.length) {
            val end = minOf(i + 200, b64.length)
            txt["t$k"] = b64.substring(i, end)
            i = end
            k += 1
        }
        val instance = "ph-" + java.util.UUID.randomUUID().toString().substring(0, 8)
        val info = WifiP2pDnsSdServiceInfo.newInstance(instance, "_photon._udp", txt)
        try {
            m.clearLocalServices(ch, null)
            m.addLocalService(ch, info, object : WifiP2pManager.ActionListener {
                override fun onSuccess() { PhotonLog.i("WFD", "advertising ${txtTokens.size}B of tokens"); pendingAdvertise = null }
                override fun onFailure(code: Int) { PhotonLog.e("WFD", "addLocalService failed, code $code") }
            })
        } catch (e: SecurityException) {
            PhotonLog.e("WFD", "advertise SecurityException: ${e.message}")
        }
    }

    /** Discover nearby `_photon._udp` services; each TXT record's reassembled token blob goes down to Rust for friend matching. */
    fun startDiscovery() {
        if (!hasPerm()) {
            pendingDiscovery = true
            PhotonLog.i("WFD", "discovery waiting on permission")
            activity?.requestWfdPermissions()
            return
        }
        val m = mgr() ?: return
        val ch = channel ?: return
        m.setDnsSdResponseListeners(ch,
            { _, _, _ -> /* service name alone carries nothing — tokens ride the TXT record */ },
            { fullDomain, txt, _ ->
                if (!fullDomain.startsWith("ph-") && !fullDomain.contains("_photon._udp")) return@setDnsSdResponseListeners
                val b64 = txt.entries.filter { it.key.startsWith("t") }
                    .sortedBy { it.key.removePrefix("t").toIntOrNull() ?: 0 }
                    .joinToString("") { it.value }
                if (b64.isEmpty()) return@setDnsSdResponseListeners
                try {
                    nativeOnServiceFound(Base64.decode(b64, Base64.NO_WRAP or Base64.NO_PADDING))
                } catch (_: IllegalArgumentException) {}
            })
        try {
            m.addServiceRequest(ch, WifiP2pDnsSdServiceRequest.newInstance(), object : WifiP2pManager.ActionListener {
                override fun onSuccess() {
                    m.discoverServices(ch, object : WifiP2pManager.ActionListener {
                        override fun onSuccess() { PhotonLog.i("WFD", "service discovery running"); pendingDiscovery = false }
                        override fun onFailure(code: Int) { PhotonLog.e("WFD", "discoverServices failed, code $code") }
                    })
                }
                override fun onFailure(code: Int) { PhotonLog.e("WFD", "addServiceRequest failed, code $code") }
            })
        } catch (e: SecurityException) {
            PhotonLog.e("WFD", "discovery SecurityException: ${e.message}")
        }
    }

    /** Stop advertise + discovery (leaves an established group alone). */
    fun stopAll() {
        pendingAdvertise = null
        pendingDiscovery = false
        val m = manager ?: return
        val ch = channel ?: return
        try {
            m.clearLocalServices(ch, null)
            m.clearServiceRequests(ch, null)
            m.stopPeerDiscovery(ch, null)
        } catch (_: SecurityException) {}
    }

    /** Pre-shared config (API 29+): both createGroup and connect skip the WPS dialog when the SSID+PSK are known. GROUP_OWNER_BAND_AUTO lets the framework co-channel with the infra STA link. */
    private fun sharedConfig(ssid: String, psk: String): WifiP2pConfig? {
        if (Build.VERSION.SDK_INT < 29) {
            PhotonLog.w("WFD", "pre-shared group config needs API 29+")
            return null
        }
        return WifiP2pConfig.Builder()
            .setNetworkName(ssid)
            .setPassphrase(psk)
            .setGroupOperatingBand(WifiP2pConfig.GROUP_OWNER_BAND_AUTO)
            .build()
    }

    /** We are the credential's designated GO: raise the group. */
    fun createGroup(ssid: String, psk: String) {
        val m = mgr() ?: return
        val ch = channel ?: return
        val cfg = sharedConfig(ssid, psk) ?: return
        try {
            m.createGroup(ch, cfg, object : WifiP2pManager.ActionListener {
                override fun onSuccess() { PhotonLog.i("WFD", "group created ($ssid)") }
                override fun onFailure(code: Int) { PhotonLog.e("WFD", "createGroup failed, code $code") }
            })
        } catch (e: SecurityException) {
            PhotonLog.e("WFD", "createGroup SecurityException: ${e.message}")
        }
    }

    /** We are the joiner: connect to the friend's group with the pre-shared credentials. */
    fun connectGroup(ssid: String, psk: String) {
        val m = mgr() ?: return
        val ch = channel ?: return
        val cfg = sharedConfig(ssid, psk) ?: return
        try {
            m.connect(ch, cfg, object : WifiP2pManager.ActionListener {
                override fun onSuccess() { PhotonLog.i("WFD", "joining group ($ssid)") }
                override fun onFailure(code: Int) { PhotonLog.e("WFD", "connect failed, code $code") }
            })
        } catch (e: SecurityException) {
            PhotonLog.e("WFD", "connect SecurityException: ${e.message}")
        }
    }

    fun removeGroup() {
        val m = manager ?: return
        val ch = channel ?: return
        try {
            m.removeGroup(ch, object : WifiP2pManager.ActionListener {
                override fun onSuccess() { PhotonLog.i("WFD", "group removed") }
                override fun onFailure(code: Int) { PhotonLog.w("WFD", "removeGroup failed, code $code") }
            })
        } catch (_: SecurityException) {}
    }
}
