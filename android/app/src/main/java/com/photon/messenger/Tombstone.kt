package com.photon.messenger

/**
 * A reader for the native-crash tombstone ApplicationExitInfo hands back (debuggerd's `Tombstone` protobuf, system/core/debuggerd/proto/tombstone.proto).
 * The bytes are binary; logging them as text mangled every frame address (2026-09-10, Nick's Scudo abort: the log held the thread name and nothing to symbolize).
 * Hand-walked protobuf — no generated code, no dependency — reading only the fields a crash report needs: the signal, the abort message, the causes, and the crashing thread's frames.
 * A frame line carries `rel_pc` + the mapped file + build id; symbolize against the unstripped .so kept per build (scripts/lib/android.sh) with `llvm-symbolizer --obj=<so> 0x<rel_pc>`.
 */
object Tombstone {
    private class Reader(val b: ByteArray, var pos: Int, val end: Int) {
        fun hasMore() = pos < end
        fun varint(): Long {
            var shift = 0
            var v = 0L
            while (pos < end && shift < 64) {
                val byte = b[pos++].toInt() and 0xFF
                v = v or ((byte and 0x7F).toLong() shl shift)
                if (byte and 0x80 == 0) return v
                shift += 7
            }
            return v
        }
        /** Returns (field number, wire type). */
        fun tag(): Pair<Int, Int> {
            val k = varint()
            return Pair((k ushr 3).toInt(), (k and 7L).toInt())
        }
        fun skip(wt: Int) {
            when (wt) {
                0 -> varint()
                1 -> pos += 8
                2 -> { val n = varint().toInt(); pos = minOf(end, pos + maxOf(0, n)) }
                5 -> pos += 4
                else -> pos = end
            }
        }
        fun sub(): Reader {
            val n = varint().toInt()
            val s = pos
            pos = minOf(end, pos + maxOf(0, n))
            return Reader(b, s, pos)
        }
        fun str(): String {
            val r = sub()
            return String(b, r.pos, r.end - r.pos, Charsets.UTF_8)
        }
    }

    private class Frame(var relPc: Long = 0, var pc: Long = 0, var fn: String = "", var fnOff: Long = 0, var file: String = "", var buildId: String = "")
    private class Thread(var id: Int = 0, var name: String = "", val frames: MutableList<Frame> = mutableListOf())

    /** Human lines for the log: the signal, the abort message, the causes, and the crashing thread's frames (at most `maxFrames`). Empty when the bytes are not a tombstone we can read. */
    fun summarize(bytes: ByteArray, maxFrames: Int = 48): List<String> {
        val out = mutableListOf<String>()
        try {
            val r = Reader(bytes, 0, bytes.size)
            var tid = -1
            var signal = ""
            var abort = ""
            val causes = mutableListOf<String>()
            val threads = mutableListOf<Thread>()
            while (r.hasMore()) {
                val (f, wt) = r.tag()
                when {
                    f == 6 && wt == 0 -> tid = r.varint().toInt()
                    f == 10 && wt == 2 -> signal = readSignal(r.sub())
                    f == 14 && wt == 2 -> abort = r.str()
                    f == 15 && wt == 2 -> readCause(r.sub())?.let { causes.add(it) }
                    f == 16 && wt == 2 -> readThreadEntry(r.sub())?.let { threads.add(it) }
                    else -> r.skip(wt)
                }
            }
            val crashing = threads.firstOrNull { it.id == tid } ?: threads.firstOrNull { it.frames.isNotEmpty() }
            out.add("tombstone: $signal on thread '${crashing?.name ?: "?"}' tid $tid (${threads.size} threads)")
            if (abort.isNotEmpty()) out.add("tombstone: abort message: $abort")
            for (c in causes) out.add("tombstone: cause: $c")
            crashing?.frames?.take(maxFrames)?.forEachIndexed { i, fr ->
                val fn = if (fr.fn.isNotEmpty()) " (${fr.fn}+${fr.fnOff})" else ""
                val bid = if (fr.buildId.isNotEmpty()) " (BuildId: ${fr.buildId})" else ""
                out.add("tombstone: #%02d pc %016x %s%s%s".format(i, fr.relPc, fr.file, fn, bid))
            }
        } catch (_: Exception) {
        }
        return out
    }

    private fun readSignal(r: Reader): String {
        var number = 0
        var name = ""
        var code = 0
        var codeName = ""
        var fault = -1L
        while (r.hasMore()) {
            val (f, wt) = r.tag()
            when {
                f == 1 && wt == 0 -> number = r.varint().toInt()
                f == 2 && wt == 2 -> name = r.str()
                f == 3 && wt == 0 -> code = r.varint().toInt()
                f == 4 && wt == 2 -> codeName = r.str()
                f == 9 && wt == 0 -> fault = r.varint()
                else -> r.skip(wt)
            }
        }
        val fa = if (fault >= 0) ", fault addr 0x%x".format(fault) else ""
        return "signal $number ($name), code $code ($codeName)$fa"
    }

    private fun readCause(r: Reader): String? {
        var text: String? = null
        while (r.hasMore()) {
            val (f, wt) = r.tag()
            if (f == 1 && wt == 2) text = r.str() else r.skip(wt)
        }
        return text
    }

    /** map<uint32, Thread> entry: key = 1, value = 2. */
    private fun readThreadEntry(r: Reader): Thread? {
        var t: Thread? = null
        while (r.hasMore()) {
            val (f, wt) = r.tag()
            if (f == 2 && wt == 2) t = readThread(r.sub()) else r.skip(wt)
        }
        return t
    }

    private fun readThread(r: Reader): Thread {
        val t = Thread()
        while (r.hasMore()) {
            val (f, wt) = r.tag()
            when {
                f == 1 && wt == 0 -> t.id = r.varint().toInt()
                f == 2 && wt == 2 -> t.name = r.str()
                f == 4 && wt == 2 -> t.frames.add(readFrame(r.sub()))
                else -> r.skip(wt)
            }
        }
        return t
    }

    private fun readFrame(r: Reader): Frame {
        val fr = Frame()
        while (r.hasMore()) {
            val (f, wt) = r.tag()
            when {
                f == 1 && wt == 0 -> fr.relPc = r.varint()
                f == 2 && wt == 0 -> fr.pc = r.varint()
                f == 4 && wt == 2 -> fr.fn = r.str()
                f == 5 && wt == 0 -> fr.fnOff = r.varint()
                f == 6 && wt == 2 -> fr.file = r.str()
                f == 8 && wt == 2 -> fr.buildId = r.str()
                else -> r.skip(wt)
            }
        }
        return fr
    }
}
