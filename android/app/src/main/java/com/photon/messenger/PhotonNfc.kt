package com.photon.messenger

import android.app.Activity
import android.nfc.NfcAdapter
import android.nfc.Tag
import android.nfc.tech.IsoDep

/**
 * NFC instant device add (docs/pairing-v2.md) — both roles of the tap.
 *
 * JOINER: [startServe] hands the session's 32-byte secret S to [PhotonHceService], which serves it
 * as a "dumb tag" under photon's PRIVATE AID — only an app that explicitly SELECTs our AID reads it,
 * so a generic phone bump-reading NDEF gets nothing. [stopServe] clears it (the Rust ServeGuard drops
 * on every ceremony exit path).
 *
 * SPONSOR: [startReader] enables reader mode while the AddDevice screen is up. A tap runs the two-APDU
 * exchange (SELECT AID → GET DATA), and the 32 bytes go down to Rust via [nativeOnNfcSecret], where the
 * matcher hashes them against each candidate's published commitment — a match binds without typing words.
 *
 * Registered like PhotonBeacon: [init] from PhotonActivity.onCreate caches the JNI bridge.
 */
object PhotonNfc {
    /** Photon's proprietary AID (F0 = proprietary range, then "PHOTON" + 01). Must match apduservice.xml. */
    val AID: ByteArray = byteArrayOf(0xF0.toByte(), 0x50, 0x48, 0x4F, 0x54, 0x4F, 0x4E, 0x01)

    /** The secret the HCE service serves while a join ceremony is live; null = not serving (respond empty). */
    @Volatile
    var serveSecret: ByteArray? = null

    private var activity: PhotonActivity? = null

    private external fun nativeInit()
    private external fun nativeOnNfcSecret(secret: ByteArray)

    /** Called once from PhotonActivity.onCreate (after loadLibrary). */
    fun init(a: PhotonActivity) {
        activity = a
        nativeInit()
    }

    // ── Joiner role ──

    fun startServe(secret: ByteArray) {
        if (secret.size == 32) {
            serveSecret = secret
            PhotonLog.i("Nfc", "serving pairing secret over HCE")
        }
    }

    fun stopServe() {
        serveSecret = null
    }

    // ── Sponsor role ──

    private val readerCallback = NfcAdapter.ReaderCallback { tag: Tag ->
        try {
            val iso = IsoDep.get(tag) ?: run {
                PhotonLog.i("Nfc", "tapped tag has no IsoDep — ignoring")
                return@ReaderCallback
            }
            iso.connect()
            iso.timeout = 1000
            // SELECT our AID: 00 A4 04 00 Lc AID 00
            val select = byteArrayOf(0x00, 0xA4.toByte(), 0x04, 0x00, AID.size.toByte()) + AID + byteArrayOf(0x00)
            val selResp = iso.transceive(select)
            if (selResp.size < 2 || selResp[selResp.size - 2] != 0x90.toByte() || selResp[selResp.size - 1] != 0x00.toByte()) {
                PhotonLog.i("Nfc", "SELECT rejected — not a photon joiner")
                iso.close()
                return@ReaderCallback
            }
            // GET DATA: 80 CA 00 00 00 → S (32) ‖ 9000
            val getData = byteArrayOf(0x80.toByte(), 0xCA.toByte(), 0x00, 0x00, 0x00)
            val resp = iso.transceive(getData)
            iso.close()
            if (resp.size == 34 && resp[32] == 0x90.toByte() && resp[33] == 0x00.toByte()) {
                nativeOnNfcSecret(resp.copyOfRange(0, 32))
                PhotonLog.i("Nfc", "pairing secret read from tap")
            } else {
                PhotonLog.i("Nfc", "GET DATA returned unexpected shape (${resp.size} bytes)")
            }
        } catch (e: Exception) {
            PhotonLog.w("Nfc", "tap exchange failed: ${e.message}")
        }
    }

    fun startReader() {
        val a = activity ?: return
        val adapter = NfcAdapter.getDefaultAdapter(a) ?: run {
            PhotonLog.i("Nfc", "no NFC adapter — reader not started")
            return
        }
        a.runOnUiThread {
            adapter.enableReaderMode(
                a,
                readerCallback,
                NfcAdapter.FLAG_READER_NFC_A or NfcAdapter.FLAG_READER_SKIP_NDEF_CHECK,
                null,
            )
        }
        PhotonLog.i("Nfc", "reader mode on")
    }

    fun stopReader() {
        val a = activity ?: return
        val adapter = NfcAdapter.getDefaultAdapter(a) ?: return
        a.runOnUiThread { adapter.disableReaderMode(a) }
        PhotonLog.i("Nfc", "reader mode off")
    }
}
