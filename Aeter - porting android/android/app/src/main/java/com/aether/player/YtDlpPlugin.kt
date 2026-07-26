package com.aether.player

import android.content.Context
import android.util.Log
import com.getcapacitor.JSArray
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import com.yausername.ffmpeg.FFmpeg
import com.yausername.youtubedl_android.YoutubeDL
import com.yausername.youtubedl_android.YoutubeDLRequest
import java.io.File
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantReadWriteLock
import kotlin.concurrent.read
import kotlin.concurrent.write

/**
 * yt-dlp execution backed by the youtubedl-android library (yausername), which
 * bundles a Python interpreter + ffmpeg + QuickJS compiled for Android. The
 * official yt-dlp standalone binary is a PyInstaller/glibc build and cannot exec
 * on Android's bionic libc, so the nodejs-mobile backend can't spawn it directly.
 *
 * Reached from the backend via reverse-RPC: callNative('ytdlpRun', { args }) in
 * node-backend/ytdlp-shim.ts → src/lib/nativeRpc.ts → YtDlp.run(). The backend
 * passes the exact yt-dlp argv (built in electron/modules/download/); it MUST be
 * forwarded verbatim via addCommands(). addOption() must not be used here: it
 * keys options in a LinkedHashMap, which deduplicates repeated options (--print
 * appears twice in a download argv) and detaches values from their options —
 * yt-dlp then parses those orphaned values as URLs ("is not a valid URL").
 *
 * CONCURRENCY: Capacitor dispatches every @PluginMethod serially on ONE shared
 * "CapacitorPlugins" HandlerThread. A download blocks for minutes, so running
 * run() inline would (a) make every yt-dlp call serial — no real parallelism —
 * and (b) stall every other native call. We therefore offload each execute() to
 * a bounded worker pool and resolve the PluginCall from the worker, freeing the
 * Capacitor thread immediately. youtubedl-android's execute() is not internally
 * serialized, so concurrent runs are safe (bounded here by POOL_SIZE; the
 * backend also caps migration concurrency). A unique processId per run lets
 * cancel() target a single download via destroyProcessById().
 *
 * yt-dlp self-update: the yt-dlp shipped inside youtubedl-android is frozen at
 * the library's release date (0.18.1 = Nov 2024). YouTube breaks old yt-dlp
 * builds within weeks (signature/SABR changes → "HTTP Error 403: Forbidden"), so
 * we pull the latest yt-dlp from the NIGHTLY channel after init() and at most
 * once per 24h.
 *
 * PACKAGE INTEGRITY: the library's updater is NOT atomic — it deletes the
 * yt-dlp dir and then copies the new package in. An execute() overlapping the
 * replacement, a process kill mid-copy, or a truncated download that "succeeds"
 * all leave a broken zip behind, and every later run dies with
 * "zipimport.ZipImportError: bad local file header". Defenses here:
 *  - pkgLock (read/write): executes hold the read lock, init/update/repair hold
 *    the write lock → the package is never replaced under a running execute;
 *  - the 24h update timestamp is only persisted after a `--version` probe
 *    proves the freshly installed package actually loads;
 *  - a run that still hits a corrupted package triggers repairYtdlp() — delete
 *    the package, restore the bundled copy (offline-safe), re-update — and is
 *    retried once transparently before surfacing the stable YTDLP_CORRUPTED code.
 */
@CapacitorPlugin(name = "YtDlp")
class YtDlpPlugin : Plugin() {

    @Volatile
    private var initialized = false

    @Volatile
    private var updateChecked = false

    // Bounded pool: enough for the migration concurrency cap (4) plus the
    // occasional search, but small enough not to spawn too many Python
    // interpreters at once (each is memory-heavy → OOM risk on phones).
    private val executor = Executors.newFixedThreadPool(POOL_SIZE)

    // Guards the yt-dlp package on disk (see PACKAGE INTEGRITY in the class doc).
    private val pkgLock = ReentrantReadWriteLock()

    private companion object {
        const val TAG = "AETHER-YTDLP"
        const val PREFS = "aether_ytdlp"
        const val KEY_LAST_UPDATE = "last_update"
        const val ONE_DAY_MS = 24L * 60L * 60L * 1000L
        const val POOL_SIZE = 5

        // Stable code surfaced to the backend/renderer instead of a Python traceback.
        const val CORRUPT_CODE = "YTDLP_CORRUPTED"
        const val BUSY_CODE = "YTDLP_BUSY"

        // youtubedl-android internals needed for repair: where the updatable yt-dlp
        // package lives and the SharedPreferences the updater uses to decide whether
        // a release is already installed. Stable across library versions (the dir
        // shows up verbatim in the ZipImportError traceback).
        const val LIB_BASE_DIR = "youtubedl-android"
        const val LIB_YTDLP_DIR = "yt-dlp"
        const val LIB_PREFS = "youtubedl-android"
        const val LIB_KEY_VERSION = "dlpVersion"
        const val LIB_KEY_VERSION_NAME = "dlpVersionName"
    }

    private fun ensureInit() {
        if (!initialized) {
            pkgLock.write {
                if (!initialized) {
                    // init() extracts python/yt-dlp/ffmpeg from the library's assets
                    // on first run; it must complete before any execute().
                    YoutubeDL.getInstance().init(context)
                    FFmpeg.getInstance().init(context)
                    initialized = true
                }
            }
        }
        maybeUpdate()
    }

    private fun safeVersion(): String? =
        try {
            YoutubeDL.getInstance().version(context)
        } catch (e: Exception) {
            null
        }

    /** True when the failure means the yt-dlp zip package on disk is broken
     *  (interrupted/overlapped update). A user cancel surfaces as
     *  YoutubeDLException("Canceled") and must NOT match. "can't open file" is
     *  the Python interpreter's own message when the package file is missing. */
    private fun isCorruptionError(message: String?): Boolean {
        val m = message ?: return false
        return m.contains("bad local file header", ignoreCase = true) ||
            m.contains("zipimport", ignoreCase = true) ||
            m.contains("can't open file", ignoreCase = true)
    }

    /** Update yt-dlp once per process and at most once per 24h. Best-effort.
     *  Takes the write lock so the library's non-atomic updater (deleteDirectory
     *  → copyFile) can never overlap a running execute(). */
    private fun maybeUpdate() {
        if (updateChecked) return
        synchronized(this) {
            if (updateChecked) return
            updateChecked = true
        }
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val now = System.currentTimeMillis()
        if (now - prefs.getLong(KEY_LAST_UPDATE, 0L) < ONE_DAY_MS) {
            Log.i(TAG, "yt-dlp update skipped (< 24h), version=${safeVersion()}")
            return
        }
        pkgLock.write {
            try {
                Log.i(TAG, "Updating yt-dlp (channel=NIGHTLY), current version=${safeVersion()}")
                val status = YoutubeDL.getInstance().updateYoutubeDL(context, YoutubeDL.UpdateChannel.NIGHTLY)
                // Persist the 24h stamp only once the installed package proves it
                // loads — an update that wrote a truncated zip must be retried, not
                // frozen in place for a day.
                if (probeOrRepair()) {
                    prefs.edit().putLong(KEY_LAST_UPDATE, now).apply()
                }
                Log.i(TAG, "yt-dlp update status=$status, version now=${safeVersion()}")
            } catch (e: Exception) {
                // Offline / update failure → keep the current package, never crash.
                Log.w(TAG, "yt-dlp update failed (keeping current package): ${e.message}")
            }
        }
    }

    /** Cheap integrity probe: `yt-dlp --version` actually zipimports the package.
     *  On corruption → repair. Returns true when the package is healthy (possibly
     *  after repair). Caller must hold the write lock. */
    private fun probeOrRepair(): Boolean {
        val req = YoutubeDLRequest(emptyList<String>())
        req.addCommands(listOf("--version"))
        return try {
            YoutubeDL.getInstance().execute(req)
            true
        } catch (e: Exception) {
            if (!isCorruptionError(e.message)) {
                Log.w(TAG, "yt-dlp version probe failed (non-corruption): ${e.message}")
                return false
            }
            Log.w(TAG, "yt-dlp package corrupted, repairing: ${e.message}")
            repairYtdlp()
        }
    }

    /** Recover from a corrupted yt-dlp package: delete it, clear the library's
     *  installed-version prefs (or the updater would believe it is up to date and
     *  never re-download), restore the bundled copy (works offline), then
     *  best-effort update to the latest nightly. Caller must hold the write lock. */
    private fun repairYtdlp(): Boolean {
        return try {
            val ytdlpDir = File(File(context.noBackupFilesDir, LIB_BASE_DIR), LIB_YTDLP_DIR)
            ytdlpDir.deleteRecursively()
            context.getSharedPreferences(LIB_PREFS, Context.MODE_PRIVATE)
                .edit()
                .remove(LIB_KEY_VERSION)
                .remove(LIB_KEY_VERSION_NAME)
                .apply()
            YoutubeDL.getInstance().init_ytdlp(context, ytdlpDir)
            Log.i(TAG, "yt-dlp package restored from bundled copy")
            try {
                val status = YoutubeDL.getInstance().updateYoutubeDL(context, YoutubeDL.UpdateChannel.NIGHTLY)
                context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                    .edit()
                    .putLong(KEY_LAST_UPDATE, System.currentTimeMillis())
                    .apply()
                Log.i(TAG, "yt-dlp re-updated after repair: status=$status, version=${safeVersion()}")
            } catch (e: Exception) {
                // Offline: the bundled (old) yt-dlp still runs; YouTube may 403 until
                // the next successful update, but the package is loadable again.
                Log.w(TAG, "yt-dlp update after repair failed (keeping bundled): ${e.message}")
            }
            true
        } catch (e: Exception) {
            Log.e(TAG, "yt-dlp repair failed: ${e.message}")
            false
        }
    }

    /** Run one yt-dlp invocation under the package read lock and shape the result. */
    private fun executeToResult(jsArgs: JSArray, processId: String): JSObject {
        // Verbatim argv passthrough (see the class doc: addOption() would
        // dedupe repeated options and orphan their values).
        val request = YoutubeDLRequest(emptyList<String>())
        request.addCommands((0 until jsArgs.length()).map { jsArgs.getString(it) })

        // Live progress callback (progress 0..100, etaInSeconds, line):
        // push the percent straight into the download notification so the
        // bar moves in real time — even in the background, since this is a
        // native path with no WebView round-trip. Best-effort / throttled.
        var lastPct = -1
        val response = pkgLock.read {
            YoutubeDL.getInstance().execute(request, processId) { progress, _, _ ->
                val pct = progress.toInt()
                if (pct in 0..100 && pct != lastPct) {
                    lastPct = pct
                    DownloadForegroundService.applyLivePercent(context, pct)
                }
            }
        }

        val result = JSObject()
        result.put("code", response.exitCode)
        result.put("stdout", response.out)
        result.put("stderr", response.err)
        return result
    }

    @PluginMethod
    fun run(call: PluginCall) {
        val jsArgs: JSArray = call.getArray("args") ?: JSArray()
        // A stable id so cancel() can target this exact process; default to a
        // fresh UUID for callers that don't supply one (e.g. one-shot searches).
        val processId = call.getString("processId") ?: UUID.randomUUID().toString()

        // Offload off the shared Capacitor handler thread so multiple downloads
        // actually run in parallel and other native calls aren't blocked.
        executor.execute {
            try {
                ensureInit()
                call.resolve(executeToResult(jsArgs, processId))
            } catch (e: Exception) {
                if (isCorruptionError(e.message)) {
                    // Broken package on disk: repair and retry this run once,
                    // transparently. Only if that still fails does the caller see
                    // the stable CORRUPT_CODE (mapped to an i18n message).
                    val repaired = pkgLock.write { repairYtdlp() }
                    if (repaired) {
                        try {
                            call.resolve(executeToResult(jsArgs, processId))
                            return@execute
                        } catch (e2: Exception) {
                            Log.e(TAG, "yt-dlp still failing after repair: ${e2.message}")
                            call.reject(CORRUPT_CODE, e2)
                            return@execute
                        }
                    }
                    call.reject(CORRUPT_CODE, e)
                    return@execute
                }
                // A user cancellation (destroyProcessById) surfaces here as a
                // YoutubeDLException("Canceled"); report it without a stack spam.
                call.reject("yt-dlp failed: ${e.message}", e)
            }
        }
    }

    /** Terminate a running yt-dlp process started with the given processId. */
    @PluginMethod
    fun cancel(call: PluginCall) {
        val processId = call.getString("processId")
        if (processId == null) {
            call.reject("processId required")
            return
        }
        val killed =
            try {
                YoutubeDL.getInstance().destroyProcessById(processId)
            } catch (e: Exception) {
                Log.w(TAG, "yt-dlp cancel failed: ${e.message}")
                false
            }
        val result = JSObject()
        result.put("killed", killed)
        call.resolve(result)
    }

    /** Current yt-dlp version string (from the library's prefs). No side effects,
     *  no init required — safe to call from the Settings UI at any time. */
    @PluginMethod
    fun version(call: PluginCall) {
        val result = JSObject()
        result.put("version", safeVersion() ?: "")
        call.resolve(result)
    }

    /** Force a yt-dlp update now (bypasses the 24h cache). For diagnostics/UI.
     *  Waits up to 10s for running downloads to release the package; a busy
     *  queue rejects with BUSY_CODE instead of blocking the UI for minutes. */
    @PluginMethod
    fun update(call: PluginCall) {
        executor.execute {
            try {
                ensureInit()
                if (!pkgLock.writeLock().tryLock(10, TimeUnit.SECONDS)) {
                    call.reject(BUSY_CODE)
                    return@execute
                }
                try {
                    Log.i(TAG, "Forced yt-dlp update (channel=NIGHTLY), current version=${safeVersion()}")
                    val status = YoutubeDL.getInstance().updateYoutubeDL(context, YoutubeDL.UpdateChannel.NIGHTLY)
                    if (probeOrRepair()) {
                        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                            .edit()
                            .putLong(KEY_LAST_UPDATE, System.currentTimeMillis())
                            .apply()
                    }
                    val result = JSObject()
                    result.put("status", status?.toString() ?: "")
                    result.put("version", safeVersion() ?: "")
                    call.resolve(result)
                } finally {
                    pkgLock.writeLock().unlock()
                }
            } catch (e: Exception) {
                call.reject("yt-dlp update failed: ${e.message}", e)
            }
        }
    }
}
