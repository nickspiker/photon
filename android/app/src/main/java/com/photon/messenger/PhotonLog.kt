package com.photon.messenger

// Kotlin→Rust bridge into the structured VSF log (photon.log.vsf). android.util.Log / logcat is retired across the board — every Kotlin line lands in the same durable, pullable log as the Rust side, read with `photonlog`.
// Levels mirror photon's LogLevel discriminants (1=Debug, 2=Info, 3=Warn, 4=Error). Lines logged before the native network init buffer in the Rust sink and flush once the log dir is known, so even the earliest lifecycle lines survive.
object PhotonLog {
    init {
        System.loadLibrary("photon_messenger")
    }

    private external fun nativeLog(level: Int, msg: String)
    private external fun nativeFlush()

    /// Push the buffered records to disk now — the one call that must precede a death (the crash logger below).
    fun flush() = nativeFlush()

    @Volatile
    private var crashLoggerInstalled = false

    /// A JAVA-SIDE CRASH LOGS ITSELF BEFORE DYING (field 2026-09-18: "PRIOR RUN DIED reason=CRASH" with nothing in the log — reason CRASH is an uncaught Java/Kotlin exception, which the native crash handler never sees). Installed once per process, from whichever component comes up first (the Activity or the Service); the stack lands in photon.log.vsf, flushed, then the platform's own handler finishes the death as before.
    fun installCrashLogger() {
        if (crashLoggerInstalled) return
        crashLoggerInstalled = true
        val prior = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            try {
                e("Uncaught", "thread ${t.name}: ${e.stackTraceToString()}")
                flush()
            } catch (_: Throwable) {
            }
            prior?.uncaughtException(t, e)
        }
    }

    fun d(tag: String, msg: String) = nativeLog(1, "$tag: $msg")
    fun i(tag: String, msg: String) = nativeLog(2, "$tag: $msg")
    fun w(tag: String, msg: String, tr: Throwable? = null) = nativeLog(3, format(tag, msg, tr))
    fun e(tag: String, msg: String, tr: Throwable? = null) = nativeLog(4, format(tag, msg, tr))

    private fun format(tag: String, msg: String, tr: Throwable?) =
        if (tr == null) "$tag: $msg" else "$tag: $msg — $tr"
}
