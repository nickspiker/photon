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
    /** The pipeline is up: width, height, fps, the maker's XYZ→camera matrix (SENSOR_COLOR_TRANSFORM, 9 floats row-major) or null, and whether the straight-thru request took in full. */
    private val onStarted: (Int, Int, Int, FloatArray?, Boolean, Boolean) -> Unit,
    /** The self-view: a 4× subsampled I420 frame (y‖u‖v), every other camera frame. */
    private val onSelf: (ByteArray, Int, Int) -> Unit,
    /** The picture's rotation changed: quarter turns clockwise that put the world's up at the top, and whether this is a front camera. */
    private val onOrient: (Int, Boolean) -> Unit,
) {
    companion object {
        private const val TAG = "PhotonBeam"
        // CAPTURE size: the ladder's phone ceiling (beam_ladder.rs, 1280×720 = band 3). The CODED size is the ladder's band, resampled from this in Rust (beam_scale.rs).
        const val WANT_W = 1280
        const val WANT_H = 720
        /** The ladder's coded sizes, smallest first — must match beam_ladder::BANDS. */
        val BANDS = arrayOf(intArrayOf(160, 90), intArrayOf(320, 180), intArrayOf(640, 360), intArrayOf(1280, 720), intArrayOf(2560, 1440))
        const val FPS = 30
        const val BITRATE = 600_000
        /** Frames per full intra-refresh sweep: two seconds at 30 fps. */
        const val INTRA_REFRESH_FRAMES = 60
        /** Parameter sets ride the first access unit and every this many frames after (the beam's info cadence). */
        const val CONFIG_EVERY = 30
        private val UNIT_GAINS = intArrayOf(4096, 4096, 4096, 4096)
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
    @Volatile private var running = false
    private var w = WANT_W
    private var h = WANT_H
    // The encoder's coded size and bitrate (the ladder's current band) — distinct from the capture size above.
    private var codedW = 640
    private var codedH = 360
    private var codedKbps = 0
    private var tierUsed = 0
    private external fun nativeBeamPlan(timestampNs: Long): Long
    private external fun nativeBeamFill(
        sy: java.nio.ByteBuffer, su: java.nio.ByteBuffer, sv: java.nio.ByteBuffer, sYRow: Int, sYPx: Int, sUVRow: Int, sUVPx: Int, sw: Int, sh: Int,
        dy: java.nio.ByteBuffer, du: java.nio.ByteBuffer, dv: java.nio.ByteBuffer, dYRow: Int, dYPx: Int, dUVRow: Int, dUVPx: Int, dw: Int, dh: Int, soft: Int
    ): Boolean
    @Volatile private var gains: IntArray = UNIT_GAINS

    /** Open the camera and the encoder. The CAMERA permission must already be granted. */
    fun start() {
        if (running) return
        running = true
        cameraThread = HandlerThread("photon-beam-camera").also { it.start() }
        cameraHandler = Handler(cameraThread!!.looper)
        codecThread = HandlerThread("photon-beam-codec").also { it.start() }
        codecHandler = Handler(codecThread!!.looper)
        val cm = ctx.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val id = pickCamera(cm) ?: run {
            PhotonLog.w(TAG, "no camera")
            stop()
            return
        }
        val chars = cm.getCameraCharacteristics(id)
        sensorOrientation = chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
        frontFacing = chars.get(CameraCharacteristics.LENS_FACING) == CameraMetadata.LENS_FACING_FRONT
        lastQuarter = -1
        val size = pickSize(chars)
        w = size.width
        h = size.height
        val matrix = makerMatrix(chars)
        try {
            startCodec()
        } catch (e: Throwable) {
            PhotonLog.w(TAG, "encoder failed", e)
            stop()
            return
        }
        reader = ImageReader.newInstance(w, h, ImageFormat.YUV_420_888, 4).also { r ->
            r.setOnImageAvailableListener({ rd ->
                val img = rd.acquireLatestImage() ?: return@setOnImageAvailableListener
                try {
                    feed(img)
                } finally {
                    img.close()
                }
            }, cameraHandler)
        }
        try {
            cm.openCamera(id, object : CameraDevice.StateCallback() {
                override fun onOpened(dev: CameraDevice) {
                    camera = dev
                    try {
                        val straight = startSession(dev, chars)
                        // The label follows the COLOUR path alone (not whether distortion or edge keys existed): absolute only when the straight colour path took.
                        onStarted(w, h, FPS, matrix, fullStraight, curveTook)
                    } catch (e: Throwable) {
                        PhotonLog.w(TAG, "session failed", e)
                        stop()
                    }
                }
                override fun onDisconnected(dev: CameraDevice) { PhotonLog.w(TAG, "camera disconnected"); stop() }
                override fun onError(dev: CameraDevice, error: Int) { PhotonLog.w(TAG, "camera error $error"); stop() }
            }, cameraHandler)
        } catch (e: CameraAccessException) {
            PhotonLog.w(TAG, "openCamera", e)
            stop()
        } catch (e: SecurityException) {
            PhotonLog.w(TAG, "openCamera: CAMERA not granted", e)
            stop()
        }
    }

    fun stop() {
        if (!running) return
        running = false
        try { session?.close() } catch (_: Throwable) {}
        session = null
        try { camera?.close() } catch (_: Throwable) {}
        camera = null
        try { reader?.close() } catch (_: Throwable) {}
        reader = null
        try { codec?.stop(); codec?.release() } catch (_: Throwable) {}
        codec = null
        freeInputs.clear()
        config = null
        cameraThread?.quitSafely(); cameraThread = null; cameraHandler = null
        codecThread?.quitSafely(); codecThread = null; codecHandler = null
        PhotonLog.i(TAG, "stopped after $frames encoded frames, $fed fed, $dropped dropped")
        frames = 0
        dropped = 0
        fed = 0
    }

    /** The front camera — a beam is aimed at a person, and the person is behind the screen — else the first camera there is. */
    private fun pickCamera(cm: CameraManager): String? {
        val ids = cm.cameraIdList
        return ids.firstOrNull { cm.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == CameraMetadata.LENS_FACING_FRONT } ?: ids.firstOrNull()
    }

    /** 640×480 when the camera offers it for YUV_420_888, else the smallest size at or above it, else the largest below. Even dimensions only. */
    private fun pickSize(chars: CameraCharacteristics): Size {
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return Size(WANT_W, WANT_H)
        val sizes = map.getOutputSizes(ImageFormat.YUV_420_888)?.filter { it.width % 2 == 0 && it.height % 2 == 0 } ?: return Size(WANT_W, WANT_H)
        sizes.firstOrNull { it.width == WANT_W && it.height == WANT_H }?.let { return it }
        val above = sizes.filter { it.width >= WANT_W && it.height >= WANT_H }.minByOrNull { it.width * it.height }
        if (above != null) return above
        return sizes.maxByOrNull { it.width * it.height } ?: Size(WANT_W, WANT_H)
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
        setInteger(MediaFormat.KEY_FRAME_RATE, FPS)
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
                if (!freeInputs.offer(index)) {
                    // More free buffers than we track: hand it straight back empty so the codec keeps cycling.
                    try { mc.queueInputBuffer(index, 0, 0, 0, 0) } catch (_: Throwable) {}
                }
            }
            override fun onOutputBufferAvailable(mc: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
                try {
                    val buf = mc.getOutputBuffer(index) ?: return
                    val bytes = ByteArray(info.size)
                    buf.position(info.offset)
                    buf.get(bytes, 0, info.size)
                    if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                        config = bytes
                    } else if (info.size > 0) {
                        val withConfig = config != null && (frames % CONFIG_EVERY == 0L)
                        val au = if (withConfig) config!! + bytes else bytes
                        frames++
                        if (frames == 1L) PhotonLog.i(TAG, "first access unit out of the encoder: ${au.size} B (config prepended ${withConfig})")
                        onFrame(au, gains)
                        if (frames % (FPS * 10L) == 0L) PhotonLog.i(TAG, "encoded $frames frames, last ${bytes.size} B, $dropped dropped")
                    }
                } catch (e: Throwable) {
                    PhotonLog.w(TAG, "output", e)
                } finally {
                    try { mc.releaseOutputBuffer(index, false) } catch (_: Throwable) {}
                }
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

    /** The self-view inset: every other frame, the camera subsampled 4× in each direction (160×120 at 640×480), straight from the camera planes. */
    private fun sendSelf(img: Image) {
        selfTick++
        if (selfTick % 2L != 0L) return
        val sw = (w / 4) and 1.inv()
        val sh = (h / 4) and 1.inv()
        if (sw < 2 || sh < 2) return
        val cw = sw / 2
        val ch = sh / 2
        val out = ByteArray(sw * sh + 2 * cw * ch)
        val yp = img.planes[0]; val up = img.planes[1]; val vp = img.planes[2]
        val yb = yp.buffer; val ub = up.buffer; val vb = vp.buffer
        for (y in 0 until sh) for (x in 0 until sw) out[y * sw + x] = yb.get((y * 4) * yp.rowStride + (x * 4) * yp.pixelStride)
        var o = sw * sh
        for (y in 0 until ch) for (x in 0 until cw) { out[o++] = ub.get((y * 4) * up.rowStride + (x * 4) * up.pixelStride) }
        for (y in 0 until ch) for (x in 0 until cw) { out[o++] = vb.get((y * 4) * vp.rowStride + (x * 4) * vp.pixelStride) }
        onSelf(out, sw, sh)
    }

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

    private fun feed(img: Image) {
        reportOrientation()
        try { sendSelf(img) } catch (_: Throwable) {}
        // THE LADDER (beam_ladder.rs): keep or drop this frame (frame rate), which band (coded size), how soft, what bitrate.
        val plan = try { nativeBeamPlan(img.timestamp) } catch (_: Throwable) { -1L }
        if (plan < 0L || (plan and 1L) == 0L) return
        val band = ((plan shr 1) and 7L).toInt().coerceIn(0, BANDS.size - 1)
        val soft = ((plan shr 4) and 7L).toInt()
        val kbps = (plan shr 8).toInt()
        val (bw, bh) = BANDS[band][0] to BANDS[band][1]
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
        // The camera frame → the coded size at the ladder's softness, written straight into the encoder's planes (crop to 16:9, shrink, grow — beam_scale.rs).
        val s = img.planes
        val d = dst.planes
        val filled = try {
            nativeBeamFill(
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

    /** Build the session with the straight-thru request. Returns whether every key the design asks for was available and set. */
    private fun startSession(dev: CameraDevice, chars: CameraCharacteristics): Boolean {
        val surface = reader!!.surface
        val req = dev.createCaptureRequest(CameraDevice.TEMPLATE_RECORD)
        req.addTarget(surface)
        req.set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, Range(FPS, FPS))
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
        val hasMaker = (chars.get(CameraCharacteristics.SENSOR_COLOR_TRANSFORM2) ?: chars.get(CameraCharacteristics.SENSOR_COLOR_TRANSFORM1)) != null
        fullStraight = manualPost && awbModes.contains(CameraMetadata.CONTROL_AWB_MODE_OFF) && hasMaker &&
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
        val toneModes = chars.get(CameraCharacteristics.TONEMAP_AVAILABLE_TONE_MAP_MODES) ?: intArrayOf()
        val curveOk = toneModes.contains(CameraMetadata.TONEMAP_MODE_CONTRAST_CURVE)
        curveTook = curveOk
        want(CaptureRequest.TONEMAP_MODE, CameraMetadata.TONEMAP_MODE_CONTRAST_CURVE, curveOk)
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
        PhotonLog.i(TAG, "request: manualPost=$manualPost awbOff=${awbModes.contains(CameraMetadata.CONTROL_AWB_MODE_OFF)} curve=$curveOk straight=$straight size=${w}x$h")
        val straightNow = straight
        @Suppress("DEPRECATION")
        dev.createCaptureSession(listOf(surface), object : CameraCaptureSession.StateCallback() {
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
                            }
                        }
                    }, cameraHandler)
                } catch (e: Throwable) {
                    PhotonLog.w(TAG, "setRepeatingRequest", e)
                }
            }
            override fun onConfigureFailed(s: CameraCaptureSession) { PhotonLog.w(TAG, "session configure failed"); stop() }
        }, cameraHandler)
        return straight
    }
}
