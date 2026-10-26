package com.photon.messenger

import android.content.Context
import android.graphics.ImageFormat
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CameraMetadata
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.hardware.camera2.params.ColorSpaceTransform
import android.hardware.camera2.params.RggbChannelVector
import android.hardware.camera2.params.TonemapCurve
import android.media.Image
import android.media.ImageReader
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.util.Range
import android.util.Size
import java.util.concurrent.ArrayBlockingQueue
import kotlin.math.sqrt

/**
 * The phone's half of a BEAM (docs/beams.md stage 3): Camera2 with the ISP stood aside, MediaCodec H.264 with no periodic keyframes, encoded access units handed to Rust.
 *
 * THE STRAIGHT-THRU REQUEST (docs/beams.md §3), each key gated on the camera's own availability lists: AWB off, colour correction in TRANSFORM_MATRIX mode with unit gains and an identity matrix, our own γ2 tonemap curve, lens shading / noise reduction / edge / aberration / distortion / stabilization / effects / scene modes OFF. Auto exposure and hot-pixel correction stay ON (level is not colour; a dead pixel in a beam is only ugly). What comes out is demosaiced SENSOR RGB under γ2 in the ISP's YCbCr — tungsten looks yellow, on purpose. The CaptureResult echoes what the HAL actually applied, and [straight] reports whether every key took; when it did not, Rust labels the frames `creative`/`assumed` instead of `absolute`/`model`.
 *
 * THE ENCODER: constant bitrate, low latency, keyframe interval NEVER (only the first frame), intra refresh so the whole picture is re-coded in a rolling stripe every [INTRA_REFRESH_FRAMES] frames. SPS/PPS arrive once as a config buffer; they are prepended to the first access unit and again every [CONFIG_EVERY] frames so a receiver that started late can still decode without a keyframe ever being asked for.
 *
 * Threads: the camera delivers on its own handler thread; the codec runs in async mode on another; a frame is copied into a codec input buffer only when one is free, otherwise dropped (the camera is never blocked).
 */
class PhotonBeam(
    private val ctx: Context,
    /** One encoded access unit + the gains the ISP applied (Q12: R, G-even, G-odd, B). */
    private val onFrame: (ByteArray, IntArray) -> Unit,
    /** The pipeline is up (again, after a camera switch): width, height, fps, the maker's XYZ→camera matrix (SENSOR_COLOR_TRANSFORM, 9 floats row-major) or null, whether the straight-thru colour path took, whether our γ2 curve took, the camera's key ("1", "0/3") and label, and whether this camera runs HDR (10-bit HLG → g2t, docs/beams.md). */
    private val onStarted: (Int, Int, Int, FloatArray?, Boolean, Boolean, String, String, Boolean) -> Unit,
    /** The self-view: a 4× subsampled I420 frame (y‖u‖v), every other camera frame. */
    private val onSelf: (ByteArray, Int, Int) -> Unit,
    /** The picture's rotation changed: quarter turns clockwise that put the world's up at the top, and whether this is a front camera. */
    private val onOrient: (Int, Boolean) -> Unit,
) {
    companion object {
        private const val TAG = "PhotonBeam"
        // The CAPTURE caps ([pickCapture]); the CODED size is the ladder's band, every band an exact half of the capture (beam_ladder.rs bands_for), resampled in Rust (beam_scale.rs).
        const val MAX_PIXELS = 5_100_000L
        const val MAX_FPS = 30
        /** The self-view inset's width target: the camera is subsampled by a whole factor to about this. */
        const val SELF_W = 320
        /** The lens the last beam was on (this app run): the next beam starts there. */
        @Volatile var lastKey: String? = null
        const val BITRATE = 600_000
        /** Frames per full intra-refresh sweep: two seconds at 30 fps. */
        const val INTRA_REFRESH_FRAMES = 60
        /** Parameter sets ride the first access unit and every this many frames after (the beam's info cadence). */
        const val CONFIG_EVERY = 30
        private val UNIT_GAINS = intArrayOf(4096, 4096, 4096, 4096)

        /** Can this camera stream 10-bit HLG into a P010 reader (Settings → Beams' HDR; API 33+)? `chars` = the sensor's (its sizes), `logical` = the request capabilities. */
        fun hlgCapable(chars: CameraCharacteristics, logical: CameraCharacteristics): Boolean {
            if (Build.VERSION.SDK_INT < 33) return false
            val caps = logical.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES) ?: return false
            if (!caps.contains(CameraMetadata.REQUEST_AVAILABLE_CAPABILITIES_DYNAMIC_RANGE_TEN_BIT)) return false
            val profiles = logical.get(CameraCharacteristics.REQUEST_AVAILABLE_DYNAMIC_RANGE_PROFILES)?.supportedProfiles ?: return false
            if (!profiles.contains(android.hardware.camera2.params.DynamicRangeProfiles.HLG10)) return false
            return !chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)?.getOutputSizes(ImageFormat.YCBCR_P010).isNullOrEmpty()
        }

        /** Does any camera on this phone offer HDR beams? Asked once at startup (no permission needed to read characteristics); the settings page greys HDR out when none does. */
        fun anyHlgCamera(ctx: Context): Boolean = try {
            val cm = ctx.getSystemService(Context.CAMERA_SERVICE) as CameraManager
            cm.cameraIdList.any { id -> cm.getCameraCharacteristics(id).let { hlgCapable(it, it) } }
        } catch (e: Throwable) {
            PhotonLog.w(TAG, "HDR camera probe", e)
            false
        }
    }

    private var cameraThread: HandlerThread? = null
    private var cameraHandler: Handler? = null
    private var codecThread: HandlerThread? = null
    private var codecHandler: Handler? = null
    private var camera: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null
    private var codec: MediaCodec? = null
    private val freeInputs = ArrayBlockingQueue<Int>(8)
    private var config: ByteArray? = null
    private var frames = 0L
    private var dropped = 0L
    private var fed = 0L
    // THE SLOT LEDGER (field 2026-10-21, Emma's Samsung on c2.qti.avc.encoder: 45 fed, 898 dropped, encoded = fed — every drop was a camera frame that found no free encoder input while the encoder sat idle): when each input went in, how long its slot took to come back, the longest an output was held.
    private val queuedAtNs = LongArray(64)
    @Volatile private var slotsOffered = 0L
    @Volatile private var slotBackN = 0L
    @Volatile private var slotBackSumNs = 0L
    @Volatile private var slotBackMaxNs = 0L
    @Volatile private var holdMaxNs = 0L
    private var keptSinceLog = 0L
    @Volatile private var running = false
    private var w = 1280
    private var h = 720
    private var fps = MAX_FPS
    // The encoder's coded size and bitrate (the ladder's current band) — distinct from the capture size above. Zero = no encoder yet: the first planned frame builds it.
    private var codedW = 0
    private var codedH = 0
    private var codedKbps = 0
    private var tierUsed = 0
    private external fun nativeBeamPlan(timestampNs: Long): Long
    private external fun nativeBeamFill(
        sy: java.nio.ByteBuffer, su: java.nio.ByteBuffer, sv: java.nio.ByteBuffer, sYRow: Int, sYPx: Int, sUVRow: Int, sUVPx: Int, sw: Int, sh: Int,
        dy: java.nio.ByteBuffer, du: java.nio.ByteBuffer, dv: java.nio.ByteBuffer, dYRow: Int, dYPx: Int, dUVRow: Int, dUVPx: Int, dw: Int, dh: Int, soft: Int
    ): Boolean
    /** The HDR fill: a P010 HLG camera frame → 8-bit g2t codes at the coded size (beam_hdr.rs). */
    private external fun nativeBeamFillHdr(
        sy: java.nio.ByteBuffer, su: java.nio.ByteBuffer, sv: java.nio.ByteBuffer, sYRow: Int, sYPx: Int, sUVRow: Int, sUVPx: Int, sw: Int, sh: Int, fullRange: Boolean,
        dy: java.nio.ByteBuffer, du: java.nio.ByteBuffer, dv: java.nio.ByteBuffer, dYRow: Int, dYPx: Int, dUVRow: Int, dUVPx: Int, dw: Int, dh: Int, soft: Int, frame: Int
    ): Boolean
    /** Settings → Beams asks for HDR (and this phone can): read at each camera start. */
    private external fun nativeBeamWantHdr(): Boolean
    /** This camera's pipeline runs HDR: a P010 reader, the HLG10 profile, the camera's own colour. */
     private var hdrNow = false
    @Volatile private var gains: IntArray = UNIT_GAINS

    /** One camera the beam can be on: a logical camera, or one physical lens of a logical multi-camera (opened thru its logical id, the stream pinned to the lens). `chars` describe the SENSOR (sizes, matrix, orientation, facing); `logical` holds the request capabilities. `key` names it to Rust ("1", "0/3"), `fov` orders it (sensor width ÷ focal length — wider first). */
    private class Target(val openId: String, val physicalId: String?, val chars: CameraCharacteristics, val logical: CameraCharacteristics, val key: String, val fov: Float, val front: Boolean, val label: String) {
        val hlg: Boolean = hlgCapable(chars, logical)
    }

    private var targets: List<Target> = emptyList()
    private var target: Target? = null

    /** Open the camera and the encoder. The CAMERA permission must already be granted. */
    fun start() {
        if (running) return
        running = true
        cameraThread = HandlerThread("photon-beam-camera").also { it.start() }
        cameraHandler = Handler(cameraThread!!.looper)
        codecThread = HandlerThread("photon-beam-codec").also { it.start() }
        codecHandler = Handler(codecThread!!.looper)
        val cm = ctx.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        targets = try { listTargets(cm) } catch (e: Throwable) { PhotonLog.w(TAG, "camera list", e); emptyList() }
        if (targets.isEmpty()) {
            PhotonLog.w(TAG, "no camera")
            stop()
            return
        }
        PhotonLog.i(TAG, "cameras: " + targets.joinToString { "${it.key}${if (it.front) " front" else ""} fov ${"%.2f".format(it.fov)}" })
        // The lens used last this run, else the front camera — a beam is aimed at a person, and the person is behind the screen.
        val t = targets.firstOrNull { it.key == lastKey } ?: targets.firstOrNull { it.front } ?: targets[0]
        cameraHandler?.post { openTarget(cm, t) }
    }

    /** THE CAMERA CYCLE (Nick 2026-10-14: "tap and switch between the front and back cam, or the other three back cams… cycles thru all when I tap on my preview"): the next target, in flight. Rust keeps the beam's transport; this side restarts the camera (or only the session, between lenses of one logical camera) and the encoder, and the pipeline-up edge re-describes the beam. */
    fun switchCamera() {
        if (!running || targets.size < 2) return
        cameraHandler?.post {
            if (!running) return@post
            val cur = targets.indexOf(target).coerceAtLeast(0)
            val next = targets[(cur + 1) % targets.size]
            PhotonLog.i(TAG, "switch: ${target?.key} → ${next.key}")
            openTarget(ctx.getSystemService(Context.CAMERA_SERVICE) as CameraManager, next)
        }
    }

    /** Bring the pipeline up on `t` (camera thread). A lens of the camera already open needs only a new session; anything else reopens the device. The encoder is rebuilt lazily by the first planned frame at the size the ladder names. */
    private fun openTarget(cm: CameraManager, t: Target) {
        closeSession()
        val sameDevice = camera?.id == t.openId
        if (!sameDevice) {
            try { camera?.close() } catch (_: Throwable) {}
            camera = null
        }
        target = t
        lastKey = t.key
        sensorOrientation = t.chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
        frontFacing = t.front
        lastQuarter = -1
        // HDR when Settings → Beams asks and this lens can (an edge: read once per camera start).
        hdrNow = t.hlg && (try { nativeBeamWantHdr() } catch (_: Throwable) { false })
        val format = if (hdrNow) ImageFormat.YCBCR_P010 else ImageFormat.YUV_420_888
        val (size, rate) = pickCapture(t.chars, format)
        w = size.width
        h = size.height
        fps = rate
        val matrix = makerMatrix(t.chars)
        reader = ImageReader.newInstance(w, h, format, 4).also { r ->
            r.setOnImageAvailableListener({ rd ->
                val img = rd.acquireLatestImage() ?: return@setOnImageAvailableListener
                try {
                    feed(img)
                } finally {
                    img.close()
                }
            }, cameraHandler)
        }
        val up: (CameraDevice) -> Unit = { dev ->
            try {
                startSession(dev, t, t.physicalId != null)
                // The label follows the COLOUR path alone (not whether distortion or edge keys existed): absolute only when the straight colour path took.
                onStarted(w, h, fps, matrix, fullStraight, curveTook, t.key, t.label, hdrNow)
            } catch (e: Throwable) {
                PhotonLog.w(TAG, "session failed", e)
                stop()
            }
        }
        val dev = camera
        if (sameDevice && dev != null) {
            up(dev)
            return
        }
        try {
            cm.openCamera(t.openId, object : CameraDevice.StateCallback() {
                override fun onOpened(d: CameraDevice) {
                    if (!running) { try { d.close() } catch (_: Throwable) {}; return }
                    camera = d
                    up(d)
                }
                override fun onDisconnected(d: CameraDevice) { PhotonLog.w(TAG, "camera disconnected"); if (camera == d) stop() }
                override fun onError(d: CameraDevice, error: Int) { PhotonLog.w(TAG, "camera error $error"); if (camera == d) stop() }
            }, cameraHandler)
        } catch (e: CameraAccessException) {
            PhotonLog.w(TAG, "openCamera", e)
            stop()
        } catch (e: SecurityException) {
            PhotonLog.w(TAG, "openCamera: CAMERA not granted", e)
            stop()
        }
    }

    /** The session, the reader and the encoder — what every switch replaces. The device stays open. */
    private fun closeSession() {
        try { session?.close() } catch (_: Throwable) {}
        session = null
        try { reader?.close() } catch (_: Throwable) {}
        reader = null
        try { codec?.stop(); codec?.release() } catch (_: Throwable) {}
        codec = null
        freeInputs.clear()
        config = null
        codedW = 0
        codedH = 0
        codedKbps = 0
    }

    fun stop() {
        if (!running) return
        running = false
        closeSession()
        try { camera?.close() } catch (_: Throwable) {}
        camera = null
        target = null
        cameraThread?.quitSafely(); cameraThread = null; cameraHandler = null
        codecThread?.quitSafely(); codecThread = null; codecHandler = null
        PhotonLog.i(TAG, "stopped after $frames encoded frames, $fed fed, $dropped dropped, $slotsOffered slots offered since the last slot line")
        frames = 0
        dropped = 0
        fed = 0
    }

    /** Every camera the beam can use: each logical camera, and each physical lens of a logical multi-camera (the Pixel 8 Pro's back camera is ultrawide, main, main 2× crop, tele, tele 2× crop — the crop modes stay: Nick, "you may want to zoom in"). A lens listed twice (as its own id and inside a logical camera) counts once; a lens with no YUV stream is skipped. Front first, then widest to longest. */
    private fun listTargets(cm: CameraManager): List<Target> {
        val out = mutableListOf<Target>()
        val seen = mutableSetOf<String>()
        fun add(openId: String, physId: String?, chars: CameraCharacteristics, logical: CameraCharacteristics) {
            val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return
            if (map.getOutputSizes(ImageFormat.YUV_420_888).isNullOrEmpty()) return
            val focal = chars.get(CameraCharacteristics.LENS_INFO_AVAILABLE_FOCAL_LENGTHS)?.firstOrNull() ?: 0f
            val phys = chars.get(CameraCharacteristics.SENSOR_INFO_PHYSICAL_SIZE)
            val front = chars.get(CameraCharacteristics.LENS_FACING) == CameraMetadata.LENS_FACING_FRONT
            // The same lens and sensor area seen twice is one camera.
            val sig = "$front/${"%.2f".format(focal)}/${"%.2f".format(phys?.width ?: 0f)}"
            if (!seen.add(sig)) return
            val fov = if (focal > 0f && phys != null) phys.width / focal else 1f
            // The lens as a person names it: facing + 35 mm-equivalent focal length ("back|24"); Rust words it in the UI language for the self-view caption.
            val diag = phys?.let { kotlin.math.hypot(it.width.toDouble(), it.height.toDouble()) } ?: 0.0
            val eq = if (focal > 0f && diag > 0.0) Math.round(focal * 43.27 / diag).toInt() else 0
            val label = (if (front) "front" else "back") + "|" + eq
            out.add(Target(openId, physId, chars, logical, if (physId == null) openId else "$openId/$physId", fov, front, label))
        }
        for (id in cm.cameraIdList) {
            val chars = cm.getCameraCharacteristics(id)
            val caps = chars.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES) ?: intArrayOf()
            val physIds = if (Build.VERSION.SDK_INT >= 28 && caps.contains(CameraMetadata.REQUEST_AVAILABLE_CAPABILITIES_LOGICAL_MULTI_CAMERA)) chars.physicalCameraIds.toList() else emptyList()
            val lenses = physIds.mapNotNull { pid -> try { pid to cm.getCameraCharacteristics(pid) } catch (_: Throwable) { null } }
                .filter { (_, pc) -> !(pc.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)?.getOutputSizes(ImageFormat.YUV_420_888).isNullOrEmpty()) }
            if (lenses.size >= 2) {
                for ((pid, pc) in lenses) add(id, pid, pc, chars)
            } else {
                // One lens (or none listed): the logical camera itself, described by its one physical sensor when there is one.
                add(id, null, lenses.firstOrNull()?.second ?: chars, chars)
            }
        }
        return out.sortedWith(compareBy<Target>({ !it.front }, { -it.fov }))
    }

    /** THE CAPTURE (Nick 2026-10-14: "sampling what we have available and working with it, even if we only get a 25fps camera with a 2:1 aspect… cap the framerate at 30 and the megapixels at 5 ish… ideally uncropped"): of the YUV sizes this sensor streams, the one with the highest frame rate up to [MAX_FPS], then the least crop of the sensor's own aspect (within 1% counts as uncropped), then the most pixels up to [MAX_PIXELS]. Returns the size and the frame rate it runs at. */
    private fun pickCapture(chars: CameraCharacteristics, format: Int): Pair<Size, Int> {
        val fallback = Size(1280, 720) to MAX_FPS
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return fallback
        val sizes = map.getOutputSizes(format)?.filter { it.width % 2 == 0 && it.height % 2 == 0 && it.width.toLong() * it.height <= MAX_PIXELS } ?: return fallback
        val active = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE)
        val native = if (active != null && active.height() > 0) active.width().toFloat() / active.height() else 4f / 3f
        data class Cand(val size: Size, val fps: Int, val crop: Float)
        val cands = sizes.map { sz ->
            val dur = try { map.getOutputMinFrameDuration(format, sz) } catch (_: Throwable) { 0L }
            val rate = if (dur > 0) (1_000_000_000L / dur).toInt() else MAX_FPS
            val a = sz.width.toFloat() / sz.height
            val keep = minOf(a / native, native / a)
            Cand(sz, rate.coerceAtMost(MAX_FPS), if (keep >= 0.99f) 0f else 1f - keep)
        }
        val best = cands.sortedWith(compareBy<Cand>({ -it.fps }, { it.crop }, { -(it.size.width.toLong() * it.size.height) })).firstOrNull() ?: return fallback
        PhotonLog.i(TAG, "capture: ${best.size.width}x${best.size.height} @ ${best.fps} fps (sensor aspect ${"%.3f".format(native)}, ${if (best.crop == 0f) "uncropped" else "crop ${"%.1f".format(best.crop * 100)}%"}; ${sizes.size} sizes under ${MAX_PIXELS / 1_000_000} MP)")
        return best.size to best.fps
    }

    /** The maker's XYZ→camera matrix (DNG ColorMatrix, what lumis reads), daylight set (2) preferred. Null when the camera reports none. */
    private fun makerMatrix(chars: CameraCharacteristics): FloatArray? {
        val cst = chars.get(CameraCharacteristics.SENSOR_COLOR_TRANSFORM2) ?: chars.get(CameraCharacteristics.SENSOR_COLOR_TRANSFORM1) ?: return null
        val out = FloatArray(9)
        for (r in 0 until 3) for (c in 0 until 3) {
            val q = cst.getElement(c, r)
            out[r * 3 + c] = if (q.denominator == 0) 0f else q.numerator.toFloat() / q.denominator.toFloat()
        }
        return out
    }

    /** One encoder configuration attempt, from fullest to plainest (field 2026-10-13: Emma's phone threw IllegalArgumentException on the full set and never sent a frame). Each tier drops what an encoder is most likely to refuse; the log names the tier that took, so we learn each device's limits. */
    private fun formatFor(tier: Int): MediaFormat = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, codedW, codedH).apply {
        setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible)
        setInteger(MediaFormat.KEY_BIT_RATE, if (codedKbps > 0) codedKbps * 1000 else BITRATE)
        setInteger(MediaFormat.KEY_FRAME_RATE, fps)
        if (tier <= 1) {
            // Only the first frame is a keyframe; the picture is kept fresh by intra refresh instead.
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, -1)
            setInteger(MediaFormat.KEY_INTRA_REFRESH_PERIOD, INTRA_REFRESH_FRAMES)
        } else {
            // The plain tier: no intra refresh on this encoder, so a keyframe every two seconds — the one fallback where a burst is the price of working at all.
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 2)
        }
        if (tier <= 2) setInteger(MediaFormat.KEY_BITRATE_MODE, MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR)
        if (tier == 0) {
            setInteger(MediaFormat.KEY_LATENCY, 1)
            setInteger(MediaFormat.KEY_PRIORITY, 0)
            if (Build.VERSION.SDK_INT >= 30) setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
        }
    }

    private fun startCodec() {
        var lastErr: Throwable? = null
        for (tier in 0..3) {
            val c = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
            try {
                setCallbacks(c)
                c.configure(formatFor(tier), null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                c.start()
                codec = c
                tierUsed = tier
                PhotonLog.i(TAG, "encoder ${c.name} configured at tier $tier (0 = full low-latency + intra refresh, 1 = no latency keys, 2 = keyframes every 2 s, 3 = plainest)")
                return
            } catch (e: Throwable) {
                lastErr = e
                PhotonLog.w(TAG, "encoder ${c.name} refused tier $tier: ${e.javaClass.simpleName} ${e.message}")
                try { c.release() } catch (_: Throwable) {}
            }
        }
        throw lastErr ?: IllegalStateException("no encoder tier took")
    }

    private fun setCallbacks(c: MediaCodec) {
        c.setCallback(object : MediaCodec.Callback() {
            override fun onInputBufferAvailable(mc: MediaCodec, index: Int) {
                val at = queuedAtNs[index and 63]
                if (at != 0L) {
                    val back = System.nanoTime() - at
                    slotBackN++
                    slotBackSumNs += back
                    if (back > slotBackMaxNs) slotBackMaxNs = back
                    queuedAtNs[index and 63] = 0L
                }
                slotsOffered++
                if (!freeInputs.offer(index)) {
                    // More free buffers than we track: hand it straight back empty so the codec keeps cycling.
                    try { mc.queueInputBuffer(index, 0, 0, 0, 0) } catch (_: Throwable) {}
                }
            }
            override fun onOutputBufferAvailable(mc: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
                val t0 = System.nanoTime()
                var bytes: ByteArray? = null
                try {
                    val buf = mc.getOutputBuffer(index)
                    if (buf != null && info.size > 0) {
                        val b = ByteArray(info.size)
                        buf.position(info.offset)
                        buf.get(b, 0, info.size)
                        bytes = b
                    }
                } catch (e: Throwable) {
                    PhotonLog.w(TAG, "output", e)
                } finally {
                    // THE OUTPUT GOES BACK FIRST: an output still held can hold back the codec's next input offer, so the buffer returns before the frame goes thru JNI, RaptorQ and the seal, never after.
                    try { mc.releaseOutputBuffer(index, false) } catch (_: Throwable) {}
                }
                val held = System.nanoTime() - t0
                if (held > holdMaxNs) holdMaxNs = held
                val b = bytes ?: return
                if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                    config = b
                    return
                }
                val withConfig = config != null && (frames % CONFIG_EVERY == 0L)
                val au = if (withConfig) config!! + b else b
                frames++
                if (frames == 1L) PhotonLog.i(TAG, "first access unit out of the encoder: ${au.size} B (config prepended ${withConfig})")
                try { onFrame(au, gains) } catch (e: Throwable) { PhotonLog.w(TAG, "output", e) }
                if (frames % (fps * 10L) == 0L) PhotonLog.i(TAG, "encoded $frames frames, last ${b.size} B, $dropped dropped")
            }
            override fun onError(mc: MediaCodec, e: MediaCodec.CodecException) { PhotonLog.w(TAG, "codec error", e) }
            override fun onOutputFormatChanged(mc: MediaCodec, format: MediaFormat) { PhotonLog.i(TAG, "codec format $format") }
        }, codecHandler)
    }

    /** One camera frame into the encoder, if an input buffer is free; else dropped (never block the camera). Planes copied honouring row and pixel strides. */
    private var selfTick = 0L
    /** The full straight-thru colour path took (AWB off with our neutral gains + identity matrix). Off = the camera's own white balance and colour; no gains travel as pre-emphasis then. */
    private var fullStraight = false
    /** Our γ2 tonemap curve is in force (else the camera's own curve, treated as sRGB). */
    private var curveTook = false
    /** What the focus request chose (logged with the readback). */
    private var focusPlan = "unset"
    /** AF_MODE_AUTO needs one trigger after the repeating request starts. */
    private var oneShotAf = false
    private var sensorOrientation = 0
    private var frontFacing = true
    private var lastQuarter = -1

    /** The frames go out SENSOR-NATIVE (never turned here); this names how far the receiver turns them so the world's up is up — the sensor's mounting angle against the device's rotation (photon's gravity snap), the front camera's sense reversed (Camera2's JPEG-orientation rule). Reported on change only. */
    private fun reportOrientation() {
        val dev = PhotonActivity.deviceRotationCw
        val deg = if (frontFacing) (sensorOrientation - dev + 360) % 360 else (sensorOrientation + dev) % 360
        val q = (deg / 90) and 3
        if (q != lastQuarter) {
            lastQuarter = q
            onOrient(q, frontFacing)
            PhotonLog.i(TAG, "orientation: sensor ${sensorOrientation}°, device ${dev}°, front=$frontFacing → picture turns ${q * 90}° clockwise")
        }
    }

    /** The self-view inset: every other frame, the camera subsampled by a whole factor to about [SELF_W] wide (8× at 2560×1920 → 320×240), straight from the camera planes. */
    private fun sendSelf(img: Image) {
        selfTick++
        if (selfTick % 2L != 0L) return
        val f = (w / SELF_W).coerceAtLeast(1)
        val sw = (w / f) and 1.inv()
        val sh = (h / f) and 1.inv()
        if (sw < 2 || sh < 2) return
        val cw = sw / 2
        val ch = sh / 2
        val out = ByteArray(sw * sh + 2 * cw * ch)
        val yp = img.planes[0]; val up = img.planes[1]; val vp = img.planes[2]
        val yb = yp.buffer; val ub = up.buffer; val vb = vp.buffer
        // An HDR frame is P010: the high byte of each sample is the top 8 of its 10 bits, and video range is stretched to full. HLG's curve stands in for g2t's in the inset (both put white near three quarters of the range) — the inset only, never the wire.
        val hi = if (hdrNow) 1 else 0
        val stretch = hdrNow && !fullRange(img)
        fun ly(b: Byte): Byte { val v = b.toInt() and 0xFF; return if (stretch) ((v - 16) * 255 / 219).coerceIn(0, 255).toByte() else b }
        fun lc(b: Byte): Byte { val v = b.toInt() and 0xFF; return if (stretch) ((v - 128) * 255 / 224 + 128).coerceIn(0, 255).toByte() else b }
        for (y in 0 until sh) for (x in 0 until sw) out[y * sw + x] = ly(yb.get((y * f) * yp.rowStride + (x * f) * yp.pixelStride + hi))
        var o = sw * sh
        for (y in 0 until ch) for (x in 0 until cw) { out[o++] = lc(ub.get((y * f) * up.rowStride + (x * f) * up.pixelStride + hi)) }
        for (y in 0 until ch) for (x in 0 until cw) { out[o++] = lc(vb.get((y * f) * vp.rowStride + (x * f) * vp.pixelStride + hi)) }
        onSelf(out, sw, sh)
    }

    /** Did the camera tag this frame full range (API 33+ dataspace)? Video range otherwise — the HDR fill expands it. */
    private fun fullRange(img: Image): Boolean =
        Build.VERSION.SDK_INT >= 33 && android.hardware.DataSpace.getRange(img.dataSpace) == android.hardware.DataSpace.RANGE_FULL

    /** A band crossing: restart the encoder at the new coded size (its first frame is the keyframe the new size needs — the only one the ladder ever costs). */
    private fun reconfigure(nw: Int, nh: Int, kbps: Int) {
        val c = codec ?: return
        try { c.stop() } catch (_: Throwable) {}
        freeInputs.clear()
        config = null
        codedW = nw
        codedH = nh
        codedKbps = kbps
        try {
            setCallbacks(c)
            c.configure(formatFor(tierUsed), null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            c.start()
            PhotonLog.i(TAG, "ladder: encoder restarted at ${nw}x$nh, $kbps kbit/s")
        } catch (e: Throwable) {
            PhotonLog.w(TAG, "ladder: restart at ${nw}x$nh failed — rebuilding the encoder", e)
            try { c.release() } catch (_: Throwable) {}
            codec = null
            try { startCodec() } catch (e2: Throwable) { PhotonLog.w(TAG, "ladder: no encoder", e2) }
        }
    }

    /** Every ten seconds of KEPT frames (a frame count, never a clock): what the encoder's input slots did since the last line. */
    private fun noteKept() {
        keptSinceLog++
        if (keptSinceLog < fps * 10L) return
        val n = slotBackN
        val mean = if (n > 0) slotBackSumNs / n / 1_000_000 else -1L
        PhotonLog.i(TAG, "slots: $keptSinceLog kept, $fed fed, $dropped dropped (totals), $slotsOffered offered; a queued slot came back in $mean ms mean, ${slotBackMaxNs / 1_000_000} ms max; an output held ${holdMaxNs / 1_000} µs max; ${codec?.name} tier $tierUsed at ${codedW}x$codedH")
        keptSinceLog = 0; slotsOffered = 0; slotBackN = 0; slotBackSumNs = 0; slotBackMaxNs = 0; holdMaxNs = 0
    }

    private fun feed(img: Image) {
        reportOrientation()
        try { sendSelf(img) } catch (_: Throwable) {}
        // THE LADDER (beam_ladder.rs): keep or drop this frame (frame rate), which band (coded size), how soft, what bitrate.
        val plan = try { nativeBeamPlan(img.timestamp) } catch (_: Throwable) { -1L }
        if (plan < 0L || (plan and 1L) == 0L) return
        noteKept()
        // beam_session::pack_plan: keep | band << 1 | soft << 4 | kbit/s << 8 (14 bits) | coded width << 24 | coded height << 40 (16 bits each).
        val soft = ((plan shr 4) and 7L).toInt()
        val kbps = ((plan shr 8) and 0x3FFFL).toInt()
        val bw = ((plan shr 24) and 0xFFFFL).toInt()
        val bh = ((plan shr 40) and 0xFFFFL).toInt()
        if (bw < 2 || bh < 2) return
        if (codec == null) {
            // The first planned frame on this camera builds the encoder at the size the ladder names.
            codedW = bw
            codedH = bh
            codedKbps = kbps
            try { startCodec() } catch (e: Throwable) { PhotonLog.w(TAG, "encoder failed", e); stop() }
            return
        }
        if (bw != codedW || bh != codedH) {
            reconfigure(bw, bh, kbps)
            return
        }
        if (kbps != codedKbps && kbps > 0) {
            // Within a band: the bitrate moves LIVE — no restart, no keyframe.
            try {
                codec?.setParameters(android.os.Bundle().apply { putInt(MediaCodec.PARAMETER_KEY_VIDEO_BITRATE, kbps * 1000) })
                codedKbps = kbps
            } catch (e: Throwable) { PhotonLog.w(TAG, "ladder: bitrate change refused", e) }
        }
        val mc = codec ?: return
        val idx = freeInputs.poll() ?: run { dropped++; return }
        val dst = try { mc.getInputImage(idx) } catch (e: Throwable) { null }
        if (dst == null) {
            try { mc.queueInputBuffer(idx, 0, 0, 0, 0) } catch (_: Throwable) {}
            return
        }
        // The camera frame → the coded size at the ladder's softness, written straight into the encoder's planes (shrink, grow — beam_scale.rs; the bands share the capture's aspect, so nothing is cropped).
        val s = img.planes
        val d = dst.planes
        val filled = try {
            if (hdrNow) nativeBeamFillHdr(
                s[0].buffer, s[1].buffer, s[2].buffer, s[0].rowStride, s[0].pixelStride, s[1].rowStride, s[1].pixelStride, w, h, fullRange(img),
                d[0].buffer, d[1].buffer, d[2].buffer, d[0].rowStride, d[0].pixelStride, d[1].rowStride, d[1].pixelStride, codedW, codedH, soft, fed.toInt()
            ) else nativeBeamFill(
                s[0].buffer, s[1].buffer, s[2].buffer, s[0].rowStride, s[0].pixelStride, s[1].rowStride, s[1].pixelStride, w, h,
                d[0].buffer, d[1].buffer, d[2].buffer, d[0].rowStride, d[0].pixelStride, d[1].rowStride, d[1].pixelStride, codedW, codedH, soft
            )
        } catch (e: Throwable) { PhotonLog.w(TAG, "ladder: fill failed", e); false }
        if (!filled) {
            try { mc.queueInputBuffer(idx, 0, 0, 0, 0) } catch (_: Throwable) {}
            return
        }
        // THE LENGTH (field 2026-10-10, "stopped after 0 frames"): the frame was written thru the Image view, so the byte count to submit is the whole input buffer — the encoder reads it by the layout it chose. Submitting 0 handed it an empty buffer every frame, and it encoded nothing.
        // Strides read NOW: getInputBuffer below invalidates the input Image (field 2026-10-13, "Image is already closed" from the first-frame log line).
        val dstStrides = "${dst.planes[0].rowStride}/${dst.planes[0].pixelStride} u ${dst.planes[1].rowStride}/${dst.planes[1].pixelStride}"
        val size = try { mc.getInputBuffer(idx)?.capacity() ?: 0 } catch (e: Throwable) { 0 }
        if (size <= 0) {
            PhotonLog.w(TAG, "input buffer $idx has no capacity — frame skipped")
            try { mc.queueInputBuffer(idx, 0, 0, 0, 0) } catch (_: Throwable) {}
            return
        }
        try {
            queuedAtNs[idx and 63] = System.nanoTime()
            mc.queueInputBuffer(idx, 0, size, img.timestamp / 1000, 0)
            fed++
            if (fed == 1L) PhotonLog.i(TAG, "first frame into the encoder: ${img.width}x${img.height}, planes y ${img.planes[0].rowStride}/${img.planes[0].pixelStride} u ${img.planes[1].rowStride}/${img.planes[1].pixelStride} → input $dstStrides, $size B")
        } catch (e: Throwable) {
            PhotonLog.w(TAG, "queueInputBuffer", e)
        }
    }

    private fun copyPlane(src: Image.Plane, dst: Image.Plane, pw: Int, ph: Int) {
        val sb = src.buffer
        val db = dst.buffer
        val srs = src.rowStride
        val sps = src.pixelStride
        val drs = dst.rowStride
        val dps = dst.pixelStride
        if (sps == 1 && dps == 1) {
            val row = ByteArray(pw)
            for (y in 0 until ph) {
                sb.position(y * srs)
                sb.get(row, 0, pw)
                db.position(y * drs)
                db.put(row, 0, pw)
            }
        } else {
            for (y in 0 until ph) {
                for (x in 0 until pw) {
                    db.put(y * drs + x * dps, sb.get(y * srs + x * sps))
                }
            }
        }
    }

    /** Build the session with the straight-thru request on `t` — its lens pinned when `physical` (a lens of a logical multi-camera; a refused physical stream falls back to the logical camera). Request capabilities come from the logical camera, the colour matrix from the lens. Returns whether every key the design asks for was available and set. */
    private fun startSession(dev: CameraDevice, t: Target, physical: Boolean): Boolean {
        val chars = t.logical
        val surface = reader!!.surface
        val req = dev.createCaptureRequest(CameraDevice.TEMPLATE_RECORD)
        req.addTarget(surface)
        req.set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, aeRange(chars, fps))
        var straight = true
        fun <T> want(key: CaptureRequest.Key<T>, value: T, available: Boolean) {
            if (available) req.set(key, value) else straight = false
        }
        val caps = chars.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES) ?: intArrayOf()
        val manualPost = caps.contains(CameraMetadata.REQUEST_AVAILABLE_CAPABILITIES_MANUAL_POST_PROCESSING)
        val awbModes = chars.get(CameraCharacteristics.CONTROL_AWB_AVAILABLE_MODES) ?: intArrayOf()
        // ALL OR NOTHING ON COLOUR (field 2026-10-13, Nick: "Samsung cam looks very green… defaulting to unprofiled is not a good option"): white balance comes OFF only where our neutral gains and identity matrix can go ON in its place. A camera that lets AWB go off but lacks manual post-processing used to hand us raw, green, unbalanced sensor colour that nothing downstream undid. Otherwise the camera keeps its own white balance and colour, we read back nothing as pre-emphasis, and the beam is labelled what it is.
        // …and only where the far side can undo the raw colour: the maker's matrix must exist and our γ2 curve must take, or the frames would be unbalanced sensor colour labelled as something else.
        val toneModesEarly = chars.get(CameraCharacteristics.TONEMAP_AVAILABLE_TONE_MAP_MODES) ?: intArrayOf()
        val hasMaker = makerMatrix(t.chars) != null
        // An HDR camera keeps its own colour (the HLG10 profile carries the HAL's processing to BT.2020; no straight path, no curve of ours).
        fullStraight = !hdrNow && manualPost && awbModes.contains(CameraMetadata.CONTROL_AWB_MODE_OFF) && hasMaker &&
            toneModesEarly.contains(CameraMetadata.TONEMAP_MODE_CONTRAST_CURVE)
        if (fullStraight) {
            req.set(CaptureRequest.CONTROL_AWB_MODE, CameraMetadata.CONTROL_AWB_MODE_OFF)
            req.set(CaptureRequest.COLOR_CORRECTION_MODE, CameraMetadata.COLOR_CORRECTION_MODE_TRANSFORM_MATRIX)
            req.set(CaptureRequest.COLOR_CORRECTION_GAINS, RggbChannelVector(1f, 1f, 1f, 1f))
            req.set(CaptureRequest.COLOR_CORRECTION_TRANSFORM, ColorSpaceTransform(intArrayOf(1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1)))
        } else {
            straight = false
            req.set(CaptureRequest.CONTROL_AWB_MODE, CameraMetadata.CONTROL_AWB_MODE_AUTO)
        }
        // Our γ2 curve only on the straight path too (field 2026-10-14, Emma's Samsung, still green: we asked AWB AUTO and the readback said awb=0 — the HAL switched white balance OFF the moment it was handed a contrast curve it has no manual post-processing for). On the camera's own colour path the camera keeps its own curve as well, and the frames are labelled sRGB.
        val toneModes = chars.get(CameraCharacteristics.TONEMAP_AVAILABLE_TONE_MAP_MODES) ?: intArrayOf()
        val curveOk = fullStraight && toneModes.contains(CameraMetadata.TONEMAP_MODE_CONTRAST_CURVE)
        curveTook = curveOk
        if (fullStraight) want(CaptureRequest.TONEMAP_MODE, CameraMetadata.TONEMAP_MODE_CONTRAST_CURVE, curveOk)
        if (curveOk) {
            val n = (chars.get(CameraCharacteristics.TONEMAP_MAX_CURVE_POINTS) ?: 64).coerceIn(2, 128)
            val pts = FloatArray(n * 2)
            for (i in 0 until n) {
                val x = i.toFloat() / (n - 1)
                pts[i * 2] = x
                pts[i * 2 + 1] = sqrt(x) // γ2: out = √in
            }
            req.set(CaptureRequest.TONEMAP_CURVE, TonemapCurve(pts, pts, pts))
        }
        // Lens shading off only on the straight path: with the camera's own colour in charge, its shading map also carries the colour-shading correction, and switching it off would tint the corners.
        val shading = chars.get(CameraCharacteristics.SHADING_AVAILABLE_MODES) ?: intArrayOf()
        if (fullStraight) want(CaptureRequest.SHADING_MODE, CameraMetadata.SHADING_MODE_OFF, shading.contains(CameraMetadata.SHADING_MODE_OFF))
        val nr = chars.get(CameraCharacteristics.NOISE_REDUCTION_AVAILABLE_NOISE_REDUCTION_MODES) ?: intArrayOf()
        want(CaptureRequest.NOISE_REDUCTION_MODE, CameraMetadata.NOISE_REDUCTION_MODE_OFF, nr.contains(CameraMetadata.NOISE_REDUCTION_MODE_OFF))
        val edge = chars.get(CameraCharacteristics.EDGE_AVAILABLE_EDGE_MODES) ?: intArrayOf()
        want(CaptureRequest.EDGE_MODE, CameraMetadata.EDGE_MODE_OFF, edge.contains(CameraMetadata.EDGE_MODE_OFF))
        val aber = chars.get(CameraCharacteristics.COLOR_CORRECTION_AVAILABLE_ABERRATION_MODES) ?: intArrayOf()
        want(CaptureRequest.COLOR_CORRECTION_ABERRATION_MODE, CameraMetadata.COLOR_CORRECTION_ABERRATION_MODE_OFF, aber.contains(CameraMetadata.COLOR_CORRECTION_ABERRATION_MODE_OFF))
        if (Build.VERSION.SDK_INT >= 28) {
            val dist = chars.get(CameraCharacteristics.DISTORTION_CORRECTION_AVAILABLE_MODES) ?: intArrayOf()
            want(CaptureRequest.DISTORTION_CORRECTION_MODE, CameraMetadata.DISTORTION_CORRECTION_MODE_OFF, dist.contains(CameraMetadata.DISTORTION_CORRECTION_MODE_OFF))
        }
        // FOCUS, explicitly (field 2026-10-13, Nick: "It's focused at infinity and my face is noticeably blurry… the front cam DOES have autofocus"): the template's default is not to be trusted. CONTROL_MODE AUTO (AF modes only take effect under it), then the best autofocus the camera lists — continuous video, continuous picture, a one-shot trigger — else, for a movable lens with no autofocus, a fixed arm's length (2 diopters = 0.5 m). Focus is not colour: the straight-thru promise is untouched.
        req.set(CaptureRequest.CONTROL_MODE, CameraMetadata.CONTROL_MODE_AUTO)
        val afModes = chars.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES) ?: intArrayOf()
        val minFocus = chars.get(CameraCharacteristics.LENS_INFO_MINIMUM_FOCUS_DISTANCE) ?: 0f
        focusPlan = when {
            afModes.contains(CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_VIDEO) -> { req.set(CaptureRequest.CONTROL_AF_MODE, CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_VIDEO); "continuous-video" }
            afModes.contains(CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_PICTURE) -> { req.set(CaptureRequest.CONTROL_AF_MODE, CameraMetadata.CONTROL_AF_MODE_CONTINUOUS_PICTURE); "continuous-picture" }
            afModes.contains(CameraMetadata.CONTROL_AF_MODE_AUTO) -> { req.set(CaptureRequest.CONTROL_AF_MODE, CameraMetadata.CONTROL_AF_MODE_AUTO); oneShotAf = true; "auto (one-shot trigger)" }
            minFocus > 0f -> {
                req.set(CaptureRequest.CONTROL_AF_MODE, CameraMetadata.CONTROL_AF_MODE_OFF)
                req.set(CaptureRequest.LENS_FOCUS_DISTANCE, minOf(2f, minFocus))
                "fixed at arm's length (${minOf(2f, minFocus)} diopters)"
            }
            else -> "fixed-focus lens"
        }
        req.set(CaptureRequest.CONTROL_VIDEO_STABILIZATION_MODE, CameraMetadata.CONTROL_VIDEO_STABILIZATION_MODE_OFF)
        req.set(CaptureRequest.CONTROL_EFFECT_MODE, CameraMetadata.CONTROL_EFFECT_MODE_OFF)
        req.set(CaptureRequest.CONTROL_SCENE_MODE, CameraMetadata.CONTROL_SCENE_MODE_DISABLED)
        // AE and AF stay automatic; hot-pixel correction stays as the template has it.
        PhotonLog.i(TAG, "request: hdr=$hdrNow manualPost=$manualPost awbOff=${awbModes.contains(CameraMetadata.CONTROL_AWB_MODE_OFF)} curve=$curveOk straight=$straight size=${w}x$h")
        val straightNow = straight
        val cb = object : CameraCaptureSession.StateCallback() {
            override fun onConfigured(s: CameraCaptureSession) {
                session = s
                try {
                    // The one-shot focus (a camera with only AF_MODE_AUTO): one trigger, on the session's start edge.
                    if (oneShotAf) {
                        try {
                            val trig = dev.createCaptureRequest(CameraDevice.TEMPLATE_RECORD)
                            trig.addTarget(surface)
                            trig.set(CaptureRequest.CONTROL_MODE, CameraMetadata.CONTROL_MODE_AUTO)
                            trig.set(CaptureRequest.CONTROL_AF_MODE, CameraMetadata.CONTROL_AF_MODE_AUTO)
                            trig.set(CaptureRequest.CONTROL_AF_TRIGGER, CameraMetadata.CONTROL_AF_TRIGGER_START)
                            s.capture(trig.build(), null, cameraHandler)
                        } catch (e: Throwable) { PhotonLog.w(TAG, "one-shot focus trigger", e) }
                    }
                    s.setRepeatingRequest(req.build(), object : CameraCaptureSession.CaptureCallback() {
                        private var logged = false
                        override fun onCaptureCompleted(sess: CameraCaptureSession, request: CaptureRequest, result: TotalCaptureResult) {
                            // THE READBACK IS THE TRUTH: what the HAL applied, per frame. The gains ride every encoded frame (Q12); the rest is logged once.
                            val g = result.get(CaptureResult.COLOR_CORRECTION_GAINS)
                            // Gains travel only on the straight path (pre-emphasis the receiver divides back out). With the camera's own white balance the frames are already balanced — sending its gains would make the receiver UNDO the balance.
                            if (g != null && fullStraight) {
                                gains = intArrayOf((g.red * 4096f).toInt(), (g.greenEven * 4096f).toInt(), (g.greenOdd * 4096f).toInt(), (g.blue * 4096f).toInt())
                            }
                            if (!logged) {
                                logged = true
                                val awb = result.get(CaptureResult.CONTROL_AWB_MODE)
                                val ccm = result.get(CaptureResult.COLOR_CORRECTION_MODE)
                                val tm = result.get(CaptureResult.TONEMAP_MODE)
                                val sh = result.get(CaptureResult.SHADING_MODE)
                                val nrm = result.get(CaptureResult.NOISE_REDUCTION_MODE)
                                val afm = result.get(CaptureResult.CONTROL_AF_MODE)
                                val afs = result.get(CaptureResult.CONTROL_AF_STATE)
                                val dist = result.get(CaptureResult.LENS_FOCUS_DISTANCE)
                                PhotonLog.i(TAG, "applied: awb=$awb ccm=$ccm tonemap=$tm shading=$sh nr=$nrm gains=${g?.red},${g?.greenEven},${g?.greenOdd},${g?.blue} straightAsked=$straightNow; focus asked $focusPlan, running af=$afm state=$afs at $dist diopters")
                                if (!fullStraight && awb != CameraMetadata.CONTROL_AWB_MODE_AUTO) PhotonLog.w(TAG, "the camera's own white balance was asked for and the HAL reports awb=$awb — expect a colour cast")
                            }
                        }
                    }, cameraHandler)
                } catch (e: Throwable) {
                    PhotonLog.w(TAG, "setRepeatingRequest", e)
                }
            }
            override fun onConfigureFailed(s: CameraCaptureSession) {
                if (physical) {
                    // A HAL that will not stream this lens on its own: the logical camera instead (its own lens choice), same size and request.
                    PhotonLog.w(TAG, "session refused the physical stream ${t.key} — falling back to the logical camera ${t.openId}")
                    try { startSession(dev, t, false) } catch (e: Throwable) { PhotonLog.w(TAG, "fallback session", e); stop() }
                } else {
                    PhotonLog.w(TAG, "session configure failed")
                    stop()
                }
            }
        }
        val pinned = physical && Build.VERSION.SDK_INT >= 28 && t.physicalId != null
        if (pinned || hdrNow) {
            // THE LENS PINNED: the stream rides one physical camera of the logical one, so the picture (and its colour matrix) is that sensor's, never a HAL-chosen switch.
            // HDR: the stream asks for the HLG10 profile (10-bit, BT.2020 HLG into the P010 reader).
            val oc = android.hardware.camera2.params.OutputConfiguration(surface).apply {
                if (pinned) setPhysicalCameraId(t.physicalId)
                if (hdrNow && Build.VERSION.SDK_INT >= 33) dynamicRangeProfile = android.hardware.camera2.params.DynamicRangeProfiles.HLG10
            }
            val handler = cameraHandler ?: return straight
            val sc = android.hardware.camera2.params.SessionConfiguration(android.hardware.camera2.params.SessionConfiguration.SESSION_REGULAR, listOf(oc), java.util.concurrent.Executor { handler.post(it) }, cb)
            dev.createCaptureSession(sc)
        } else {
            @Suppress("DEPRECATION")
            dev.createCaptureSession(listOf(surface), cb, cameraHandler)
        }
        return straight
    }

    /** The auto-exposure frame-rate range for `rate`: fixed [rate, rate] when listed, else the listed range with the highest top at or below it (the highest floor among those). */
    private fun aeRange(chars: CameraCharacteristics, rate: Int): Range<Int> {
        val ranges = chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES) ?: return Range(rate, rate)
        ranges.firstOrNull { it.lower == rate && it.upper == rate }?.let { return it }
        return ranges.filter { it.upper <= rate }.maxWithOrNull(compareBy<Range<Int>>({ it.upper }, { it.lower })) ?: Range(rate, rate)
    }
}
