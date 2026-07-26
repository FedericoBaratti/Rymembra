package com.aether.player

import android.media.AudioFormat
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.os.Build
import android.os.SystemClock
import android.util.Log
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import java.io.File
import java.io.FileOutputStream
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicInteger

/**
 * Decodes a segment of an audio file to raw s16le PCM for the Shazam
 * fingerprint (electron/modules/enrichment/shazam). Reached from the node
 * backend via reverse-RPC (callNative('audioDecode', …) → src/lib/nativeRpc.ts).
 *
 * MediaExtractor seeks to the requested offset and MediaCodec decodes with the
 * OS codecs (every format the device can play — no ffmpeg binary needed). The
 * PCM is written at the file's native sample rate/channels to destPath; the
 * backend resamples to mono 16 kHz in TS (pcm.ts toMono16k), which stays
 * unit-tested and identical across platforms.
 */
@CapacitorPlugin(name = "AudioDecode")
class AudioDecodePlugin : Plugin() {

    private companion object {
        const val TAG = "AETHER-AUDIODEC"

        // Wall-clock bound on a single decode, kept well under the renderer's
        // 30s RPC timeout (nativeRpc.ts) so the native reply — success or
        // error — always arrives before either caller-side timer fires.
        const val DECODE_DEADLINE_MS = 20_000L

        // A live codec makes progress every few ms; this long with neither
        // input consumed nor output produced means it's dead. Failing fast
        // frees the single-thread executor for the queued decodes behind it.
        const val NO_PROGRESS_MS = 5_000L

        // Past this queue wait the caller has already timed out (30s renderer
        // / 40s backend): decoding anyway is wasted work that re-creates
        // destPath after the backend's cleanup unlink (pcm-shim.ts).
        const val QUEUE_STALE_MS = 25_000L
    }

    // A MediaCodec decode can take seconds; running it on Capacitor's shared
    // "CapacitorPlugins" thread starves every other plugin call (imageResize
    // etc.) past their RPC timeouts. Single thread: decode is already
    // serialized upstream (shazam.ts maxConcurrent 1), and one dedicated
    // thread caps how many decoders we hold alongside ExoPlayer's.
    private val executor = Executors.newSingleThreadExecutor()

    // Jobs waiting on the executor (including the running one). Logged on
    // entry: a missing entry log means the nrpc never reached the plugin
    // (bridge delivery), a deep queue means decodes are piling up behind a
    // slow one — the two failure modes look identical from the backend.
    private val queueDepth = AtomicInteger(0)

    @PluginMethod
    fun decode(call: PluginCall) {
        val srcPath = call.getString("srcPath")
        val destPath = call.getString("destPath")
        if (srcPath == null || destPath == null) {
            call.reject("srcPath and destPath are required")
            return
        }
        val offsetSec = call.getDouble("offsetSec") ?: 0.0
        val durationSec = call.getDouble("durationSec") ?: 12.0

        val enqueuedAt = SystemClock.elapsedRealtime()
        val depth = queueDepth.incrementAndGet()
        Log.i(TAG, "decode enqueued: ${File(srcPath).name} (depth=$depth)")
        try {
            executor.execute {
                queueDepth.decrementAndGet()
                val waitedMs = SystemClock.elapsedRealtime() - enqueuedAt
                if (waitedMs > QUEUE_STALE_MS) {
                    Log.w(TAG, "decode stale in queue (${waitedMs}ms), skipped: ${File(srcPath).name}")
                    call.reject("decode stale in queue (${waitedMs}ms)")
                    return@execute
                }
                val startMs = SystemClock.elapsedRealtime()
                try {
                    val (sampleRate, channels) = decodePcm(srcPath, destPath, offsetSec, durationSec)
                    Log.i(
                        TAG,
                        "decode ok in ${SystemClock.elapsedRealtime() - startMs}ms: ${File(srcPath).name}"
                    )
                    val ret = JSObject()
                    ret.put("sampleRate", sampleRate)
                    ret.put("channels", channels)
                    call.resolve(ret)
                } catch (e: Exception) {
                    // Drop the partial PCM: the backend unlinks destPath on
                    // failure too, but if this reply loses the race with its
                    // timeout the file would stay orphaned in the tmpdir.
                    runCatching { File(destPath).delete() }
                    Log.w(
                        TAG,
                        "decode failed after ${SystemClock.elapsedRealtime() - startMs}ms: ${e.message}"
                    )
                    call.reject("decode failed: ${e.message}", e)
                }
            }
        } catch (e: RejectedExecutionException) {
            queueDepth.decrementAndGet()
            call.reject("decode rejected: plugin shutting down")
        }
    }

    override fun handleOnDestroy() {
        executor.shutdownNow()
    }

    /**
     * Creates, configures and starts a decoder, retrying transient init
     * failures ("config failed => CORRUPTED") that occur when the device's
     * codec instances are exhausted — e.g. while ExoPlayer holds two live
     * decoders (main + crossfade tail player).
     */
    private fun createStartedCodec(mime: String, format: MediaFormat): MediaCodec {
        var lastErr: Exception? = null
        for (attempt in 1..3) {
            var c: MediaCodec? = null
            try {
                c = MediaCodec.createDecoderByType(mime)
                c.configure(format, null, null, 0)
                c.start()
                if (attempt > 1) Log.i(TAG, "codec init succeeded on attempt $attempt for $mime")
                return c
            } catch (e: Exception) {
                lastErr = e
                try {
                    c?.release()
                } catch (ignored: Exception) {
                }
                Log.w(TAG, "codec init attempt $attempt/3 failed for $mime: ${e.message}")
                if (attempt < 3) Thread.sleep(attempt * 250L)
            }
        }
        throw IllegalStateException("codec init failed after 3 attempts: ${lastErr?.message}", lastErr)
    }

    private fun decodePcm(
        srcPath: String,
        destPath: String,
        offsetSec: Double,
        durationSec: Double
    ): Pair<Int, Int> {
        val extractor = MediaExtractor()
        var codec: MediaCodec? = null
        try {
            extractor.setDataSource(srcPath)
            var trackIndex = -1
            var format: MediaFormat? = null
            for (i in 0 until extractor.trackCount) {
                val f = extractor.getTrackFormat(i)
                val mime = f.getString(MediaFormat.KEY_MIME) ?: continue
                if (mime.startsWith("audio/")) {
                    trackIndex = i
                    format = f
                    break
                }
            }
            if (trackIndex < 0 || format == null) {
                throw IllegalArgumentException("no audio track in $srcPath")
            }
            val mime = format.getString(MediaFormat.KEY_MIME)!!
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                format.setInteger(MediaFormat.KEY_PCM_ENCODING, AudioFormat.ENCODING_PCM_16BIT)
            }

            extractor.selectTrack(trackIndex)
            extractor.seekTo((offsetSec * 1_000_000).toLong(), MediaExtractor.SEEK_TO_CLOSEST_SYNC)

            codec = createStartedCodec(mime, format)

            var sampleRate = format.getInteger(MediaFormat.KEY_SAMPLE_RATE)
            var channels = format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
            var maxBytes = (durationSec * sampleRate * channels * 2).toLong()
            var written = 0L
            var inputDone = false
            var outputDone = false
            val bufferInfo = MediaCodec.BufferInfo()

            // The drain loop must be wall-clock bounded: a codec that never
            // reaches EOS (truncated file, starved codec instance) would
            // otherwise spin here forever and clog the single-thread executor.
            val startMs = SystemClock.elapsedRealtime()
            var lastProgressMs = startMs

            FileOutputStream(destPath).use { out ->
                while (!outputDone && written < maxBytes) {
                    val now = SystemClock.elapsedRealtime()
                    if (now - startMs > DECODE_DEADLINE_MS) {
                        throw IllegalStateException(
                            "decode deadline exceeded (${now - startMs}ms, ${written}B written)"
                        )
                    }
                    if (now - lastProgressMs > NO_PROGRESS_MS) {
                        throw IllegalStateException(
                            "codec made no progress for ${now - lastProgressMs}ms (${written}B written)"
                        )
                    }
                    if (!inputDone) {
                        val inIndex = codec.dequeueInputBuffer(10_000)
                        if (inIndex >= 0) {
                            val inBuf = codec.getInputBuffer(inIndex)!!
                            val size = extractor.readSampleData(inBuf, 0)
                            if (size < 0) {
                                codec.queueInputBuffer(
                                    inIndex, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM
                                )
                                inputDone = true
                            } else {
                                codec.queueInputBuffer(inIndex, 0, size, extractor.sampleTime, 0)
                                extractor.advance()
                            }
                            lastProgressMs = SystemClock.elapsedRealtime()
                        }
                    }

                    val outIndex = codec.dequeueOutputBuffer(bufferInfo, 10_000)
                    if (outIndex >= 0) {
                        if (bufferInfo.size > 0) {
                            val outBuf = codec.getOutputBuffer(outIndex)!!
                            val chunk = ByteArray(bufferInfo.size)
                            outBuf.position(bufferInfo.offset)
                            outBuf.get(chunk)
                            val toWrite = minOf(chunk.size.toLong(), maxBytes - written).toInt()
                            out.write(chunk, 0, toWrite)
                            written += toWrite
                        }
                        codec.releaseOutputBuffer(outIndex, false)
                        if (bufferInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) {
                            outputDone = true
                        }
                        lastProgressMs = SystemClock.elapsedRealtime()
                    } else if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                        // Decoders may report the real rate/channels only here.
                        val outFormat = codec.outputFormat
                        sampleRate = outFormat.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                        channels = outFormat.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
                        maxBytes = (durationSec * sampleRate * channels * 2).toLong()
                        lastProgressMs = SystemClock.elapsedRealtime()
                    }
                }
            }

            if (written == 0L) throw IllegalStateException("no PCM decoded from $srcPath")
            return Pair(sampleRate, channels)
        } finally {
            try {
                codec?.stop()
            } catch (ignored: Exception) {
            }
            codec?.release()
            extractor.release()
        }
    }
}
