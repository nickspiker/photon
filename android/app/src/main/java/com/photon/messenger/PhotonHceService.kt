package com.photon.messenger

import android.nfc.cardemulation.HostApduService
import android.os.Bundle

/**
 * The joiner's "dumb tag": serves the live pairing secret S under photon's private AID
 * (see [PhotonNfc.AID] + res/xml/apduservice.xml). Two-command state machine:
 *   SELECT AID  (00 A4 04 00 …)  → 9000
 *   GET DATA    (80 CA 00 00 00) → S(32) ‖ 9000    — only while a join ceremony is live
 * Anything else → 6D00 (instruction not supported); GET DATA with no live ceremony → 6A82 (not found).
 * The secret itself carries no authority — the bind still requires the sponsor's signed consent flow;
 * S only proves proximity to the phone whose PUBLISHED request committed to it.
 */
class PhotonHceService : HostApduService() {
    override fun processCommandApdu(apdu: ByteArray, extras: Bundle?): ByteArray {
        // SELECT (CLA=00 INS=A4): the OS routes it to us only for OUR AID, so just acknowledge.
        if (apdu.size >= 4 && apdu[0] == 0x00.toByte() && apdu[1] == 0xA4.toByte()) {
            return byteArrayOf(0x90.toByte(), 0x00)
        }
        // GET DATA (CLA=80 INS=CA): the secret, if a ceremony is live.
        if (apdu.size >= 2 && apdu[0] == 0x80.toByte() && apdu[1] == 0xCA.toByte()) {
            val s = PhotonNfc.serveSecret
            return if (s != null && s.size == 32) {
                s + byteArrayOf(0x90.toByte(), 0x00)
            } else {
                byteArrayOf(0x6A.toByte(), 0x82.toByte())
            }
        }
        return byteArrayOf(0x6D.toByte(), 0x00)
    }

    override fun onDeactivated(reason: Int) {
        // Field lost / peer deselected — nothing to clean up; the secret's lifetime is the ceremony's (ServeGuard).
    }
}
