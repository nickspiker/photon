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
    private val onStarted: (Int, Int, Int, FloatArray?, Boolean) -> Unit,
    /** The self-view: a 4× subsampled I420 frame (y‖u‖v), every other camera frame. */
    private val onSelf: (ByteArray, Int, Int) -> Unit,
) {
    companion object {
        private const val TAG = "PhotonBeam"
        const val WANT_W = 640
        const val WANT_H = 480
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
                        onStarted(w, h, FPS, matrix, straight)
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

    private fun startCodec() {
        val fmt = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, w, h).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible)
            setInteger(MediaFormat.KEY_BIT_RATE, BITRATE)
            setInteger(MediaFormat.KEY_FRAME_RATE, FPS)
            // Only the first frame is a keyframe (a negative interval, API 25+); the picture is kept fresh by intra refresh instead.
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, -1)
            setInteger(MediaFormat.KEY_INTRA_REFRESH_PERIOD, INTRA_REFRESH_FRAMES)
            setInteger(MediaFormat.KEY_BITRATE_MODE, MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR)
            setInteger(MediaFormat.KEY_LATENCY, 1)
            setInteger(MediaFormat.KEY_PRIORITY, 0)
            if (Build.VERSION.SDK_INT >= 30) setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
        }
        val c = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
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
        c.configure(fmt, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        c.start()
        codec = c
    }

    /** One camera frame into the encoder, if an input buffer is free; else dropped (never block the camera). Planes copied honouring row and pixel strides. */
    private var selfTick = 0L

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

    private fun feed(img: Image) {
        try { sendSelf(img) } catch (_: Throwable) {}
        val mc = codec ?: return
        val idx = freeInputs.poll() ?: run { dropped++; return }
        val dst = try { mc.getInputImage(idx) } catch (e: Throwable) { null }
        if (dst == null) {
            try { mc.queueInputBuffer(idx, 0, 0, 0, 0) } catch (_: Throwable) {}
            return
        }
        for (p in 0 until 3) {
            copyPlane(img.planes[p], dst.planes[p], if (p == 0) w else w / 2, if (p == 0) h else h / 2)
        }
        // THE LENGTH (field 2026-10-10, "stopped after 0 frames"): the frame was written thru the Image view, so the byte count to submit is the whole input buffer — the encoder reads it by the layout it chose. Submitting 0 handed it an empty buffer every frame, and it encoded nothing.
        val size = try { mc.getInputBuffer(idx)?.capacity() ?: 0 } catch (e: Throwable) { 0 }
        if (size <= 0) {
            PhotonLog.w(TAG, "input buffer $idx has no capacity — frame skipped")
            try { mc.queueInputBuffer(idx, 0, 0, 0, 0) } catch (_: Throwable) {}
            return
        }
        try {
            mc.queueInputBuffer(idx, 0, size, img.timestamp / 1000, 0)
            fed++
            if (fed == 1L) PhotonLog.i(TAG, "first frame into the encoder: ${img.width}x${img.height}, planes y ${img.planes[0].rowStride}/${img.planes[0].pixelStride} u ${img.planes[1].rowStride}/${img.planes[1].pixelStride} → input ${dst.planes[0].rowStride}/${dst.planes[0].pixelStride} u ${dst.planes[1].rowStride}/${dst.planes[1].pixelStride}, $size B")
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
        want(CaptureRequest.CONTROL_AWB_MODE, CameraMetadata.CONTROL_AWB_MODE_OFF, awbModes.contains(CameraMetadata.CONTROL_AWB_MODE_OFF))
        want(CaptureRequest.COLOR_CORRECTION_MODE, CameraMetadata.COLOR_CORRECTION_MODE_TRANSFORM_MATRIX, manualPost)
        want(CaptureRequest.COLOR_CORRECTION_GAINS, RggbChannelVector(1f, 1f, 1f, 1f), manualPost)
        want(CaptureRequest.COLOR_CORRECTION_TRANSFORM, ColorSpaceTransform(intArrayOf(1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1)), manualPost)
        val toneModes = chars.get(CameraCharacteristics.TONEMAP_AVAILABLE_TONE_MAP_MODES) ?: intArrayOf()
        val curveOk = toneModes.contains(CameraMetadata.TONEMAP_MODE_CONTRAST_CURVE)
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
        val shading = chars.get(CameraCharacteristics.SHADING_AVAILABLE_MODES) ?: intArrayOf()
        want(CaptureRequest.SHADING_MODE, CameraMetadata.SHADING_MODE_OFF, shading.contains(CameraMetadata.SHADING_MODE_OFF))
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
                    s.setRepeatingRequest(req.build(), object : CameraCaptureSession.CaptureCallback() {
                        private var logged = false
                        override fun onCaptureCompleted(sess: CameraCaptureSession, request: CaptureRequest, result: TotalCaptureResult) {
                            // THE READBACK IS THE TRUTH: what the HAL applied, per frame. The gains ride every encoded frame (Q12); the rest is logged once.
                            val g = result.get(CaptureResult.COLOR_CORRECTION_GAINS)
                            if (g != null) {
                                gains = intArrayOf((g.red * 4096f).toInt(), (g.greenEven * 4096f).toInt(), (g.greenOdd * 4096f).toInt(), (g.blue * 4096f).toInt())
                            }
                            if (!logged) {
                                logged = true
                                val awb = result.get(CaptureResult.CONTROL_AWB_MODE)
                                val ccm = result.get(CaptureResult.COLOR_CORRECTION_MODE)
                                val tm = result.get(CaptureResult.TONEMAP_MODE)
                                val sh = result.get(CaptureResult.SHADING_MODE)
                                val nrm = result.get(CaptureResult.NOISE_REDUCTION_MODE)
                                PhotonLog.i(TAG, "applied: awb=$awb ccm=$ccm tonemap=$tm shading=$sh nr=$nrm gains=${g?.red},${g?.greenEven},${g?.greenOdd},${g?.blue} straightAsked=$straightNow")
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
