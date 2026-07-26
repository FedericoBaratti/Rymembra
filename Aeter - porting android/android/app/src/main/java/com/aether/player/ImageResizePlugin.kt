package com.aether.player

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Base64
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import kotlin.math.max
import kotlin.math.roundToInt

/**
 * Cover-art resizing, replacing the `sharp` passthrough shim
 * (node-backend/sharp-shim.ts). Reached from the backend via reverse-RPC
 * (callNative('imageResize', …) → src/lib/nativeRpc.ts).
 *
 * Mirrors sharp's `fit: 'cover'` (scale to fill, then center-crop) and encodes
 * to WebP, matching electron/modules/coverArt.ts (512px @82, 64px @70).
 */
@CapacitorPlugin(name = "ImageResize")
class ImageResizePlugin : Plugin() {

    // Capacitor runs every @PluginMethod on one shared "CapacitorPlugins"
    // thread; decoding on it starves all other plugin calls past their RPC
    // timeouts (nativeRpc.ts). Bounded at 3: with inSampleSize the decoded
    // bitmaps are near target size, so peak memory stays capped while the
    // scan's 8-wide cover pipeline can overlap.
    private val executor = Executors.newFixedThreadPool(3)

    @PluginMethod
    fun resize(call: PluginCall) {
        val srcPath = call.getString("srcPath")
        val destPath = call.getString("destPath")

        if (srcPath == null || destPath == null) {
            call.reject("srcPath and destPath are required")
            return
        }

        val targetW = call.getInt("width") ?: 0
        val targetH = call.getInt("height") ?: 0
        val quality = call.getInt("quality") ?: 80

        try {
            executor.execute {
                try {
                    doResize(srcPath, destPath, targetW, targetH, quality)
                    call.resolve()
                } catch (e: Exception) {
                    call.reject("resize failed: ${e.message}", e)
                }
            }
        } catch (e: RejectedExecutionException) {
            call.reject("resize rejected: plugin shutting down")
        }
    }

    private fun doResize(srcPath: String, destPath: String, targetW: Int, targetH: Int, quality: Int) {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(srcPath, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) {
            throw IllegalArgumentException("undecodable image at $srcPath")
        }

        val wantW = if (targetW > 0) targetW else bounds.outWidth
        val wantH = if (targetH > 0) targetH else bounds.outHeight
        // Largest power-of-2 sample keeping BOTH dims >= target, so coverCrop
        // still only downscales (matches sharp's withoutEnlargement).
        var sample = 1
        while (bounds.outWidth / (sample * 2) >= wantW &&
            bounds.outHeight / (sample * 2) >= wantH
        ) sample *= 2

        val src = BitmapFactory.decodeFile(srcPath, BitmapFactory.Options().apply { inSampleSize = sample })
            ?: throw IllegalArgumentException("undecodable image at $srcPath")

        val outW = if (targetW > 0) targetW else src.width
        val outH = if (targetH > 0) targetH else src.height
        val cropped = coverCrop(src, outW, outH)

        @Suppress("DEPRECATION")
        val format =
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R)
                Bitmap.CompressFormat.WEBP_LOSSY
            else Bitmap.CompressFormat.WEBP

        java.io.FileOutputStream(destPath).use { out ->
            cropped.compress(format, quality, out)
        }

        if (cropped != src) cropped.recycle()
        src.recycle()
    }

    /**
     * Reads image dimensions without a full decode (inJustDecodeBounds), so the
     * backend's cover gate (size/aspect-ratio) works on Android too.
     */
    @PluginMethod
    fun probe(call: PluginCall) {
        val srcPath = call.getString("srcPath")
        if (srcPath == null) {
            call.reject("srcPath is required")
            return
        }
        try {
            executor.execute {
                try {
                    val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    BitmapFactory.decodeFile(srcPath, opts)
                    if (opts.outWidth <= 0 || opts.outHeight <= 0) {
                        throw IllegalArgumentException("undecodable image at $srcPath")
                    }
                    val ret = JSObject()
                    ret.put("width", opts.outWidth)
                    ret.put("height", opts.outHeight)
                    call.resolve(ret)
                } catch (e: Exception) {
                    call.reject("probe failed: ${e.message}", e)
                }
            }
        } catch (e: RejectedExecutionException) {
            call.reject("probe rejected: plugin shutting down")
        }
    }

    override fun handleOnDestroy() {
        executor.shutdown()
    }

    /** Scale to fill outW×outH preserving aspect, then center-crop. */
    private fun coverCrop(src: Bitmap, outW: Int, outH: Int): Bitmap {
        val scale = max(outW.toFloat() / src.width, outH.toFloat() / src.height)
        val scaledW = (src.width * scale).roundToInt()
        val scaledH = (src.height * scale).roundToInt()
        val scaled = Bitmap.createScaledBitmap(src, scaledW, scaledH, true)
        val x = ((scaledW - outW) / 2).coerceAtLeast(0)
        val y = ((scaledH - outH) / 2).coerceAtLeast(0)
        val cropW = outW.coerceAtMost(scaledW)
        val cropH = outH.coerceAtMost(scaledH)
        val cropped = Bitmap.createBitmap(scaled, x, y, cropW, cropH)
        if (scaled != cropped && scaled != src) scaled.recycle()
        return cropped
    }
}
