package com.photon.messenger

// Kotlin→Rust bridge into the structured VSF log (photon.log.vsf). android.util.Log / logcat is retired across the board — every Kotlin line lands in the same durable, pullable log as the Rust side, read with `photonlog`.
// Levels mirror photon's LogLevel discriminants (1=Debug, 2=Info, 3=Warn, 4=Error). Lines logged before the native network init buffer in the Rust sink and flush once the log dir is known, so even the earliest lifecycle lines survive.
object PhotonLog {
    init {
        System.loadLibrary("photon_messenger")
    }

    private external fun nativeLog(level: Int, msg: String)

    fun d(tag: String, msg: String) = nativeLog(1, "$tag: $msg")
    fun i(tag: String, msg: String) = nativeLog(2, "$tag: $msg")
    fun w(tag: String, msg: String, tr: Throwable? = null) = nativeLog(3, format(tag, msg, tr))
    fun e(tag: String, msg: String, tr: Throwable? = null) = nativeLog(4, format(tag, msg, tr))

    private fun format(tag: String, msg: String, tr: Throwable?) =
        if (tr == null) "$tag: $msg" else "$tag: $msg — $tr"
}
