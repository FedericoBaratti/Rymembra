package com.aether.player

import android.content.Context
import android.os.Build
import android.os.PowerManager
import android.util.Log
import java.util.concurrent.CopyOnWriteArrayList

/**
 * Process-wide thermal status watcher.
 *
 * Wraps PowerManager.addThermalStatusListener (API 29+) and maps the OS status
 * to the coarse three-level scheme shared with the node backend
 * (electron/modules/adaptiveConcurrency.ts):
 *
 *   NONE / LIGHT      → "normal"
 *   MODERATE          → "warning"
 *   SEVERE and above  → "critical"
 *
 * ThermalPlugin forwards level changes to the WebView (which relays them to the
 * node backend); NativeAudioPlugin reads [currentLevel] directly to throttle
 * its progress ticker and skip crossfade prep while critical.
 *
 * On devices without the thermal API (API < 29) [start] is a no-op and the
 * level stays "normal" forever — every consumer must (and does) treat the
 * absence of thermal data as normal operation.
 */
object ThermalMonitor {
    const val LEVEL_NORMAL = "normal"
    const val LEVEL_WARNING = "warning"
    const val LEVEL_CRITICAL = "critical"

    private const val TAG = "ThermalMonitor"

    @Volatile
    var currentLevel: String = LEVEL_NORMAL
        private set

    // Listener callbacks run on the main thread (PowerManager's default executor).
    private val listeners = CopyOnWriteArrayList<(String) -> Unit>()
    private var powerManager: PowerManager? = null
    private var started = false

    /** Idempotent; safe to call from every plugin that depends on the monitor. */
    @Synchronized
    fun start(context: Context) {
        if (started) return
        started = true
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            Log.i(TAG, "no thermal API on SDK ${Build.VERSION.SDK_INT}; level pinned to normal")
            return
        }
        try {
            val pm = context.applicationContext.getSystemService(Context.POWER_SERVICE) as PowerManager
            powerManager = pm
            onStatus(pm.currentThermalStatus)
            pm.addThermalStatusListener { status -> onStatus(status) }
        } catch (e: Exception) {
            // A broken vendor HAL must never take the app down — stay at normal.
            Log.w(TAG, "thermal listener unavailable: ${e.message}")
        }
    }

    /**
     * PowerManager.getThermalHeadroom sample (API 30+; 1.0 ≈ severe throttling
     * point), or null where unsupported / rate-limited (the OS returns NaN when
     * polled more than ~once per second). Diagnostic only — policy is driven by
     * the status listener.
     */
    fun headroom(): Float? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
        val value = try {
            powerManager?.getThermalHeadroom(15)
        } catch (_: Exception) {
            null
        }
        return if (value == null || value.isNaN()) null else value
    }

    /** Subscribe to level changes. Returns the unsubscribe function. */
    fun addListener(fn: (String) -> Unit): () -> Unit {
        listeners.add(fn)
        return { listeners.remove(fn) }
    }

    private fun onStatus(status: Int) {
        val level = when {
            status >= PowerManager.THERMAL_STATUS_SEVERE -> LEVEL_CRITICAL
            status == PowerManager.THERMAL_STATUS_MODERATE -> LEVEL_WARNING
            else -> LEVEL_NORMAL
        }
        if (level == currentLevel) return
        currentLevel = level
        Log.i(TAG, "status=$status level=$level" + (headroom()?.let { " headroom=$it" } ?: ""))
        for (listener in listeners) {
            try {
                listener(level)
            } catch (e: Exception) {
                Log.w(TAG, "listener failed: ${e.message}")
            }
        }
    }
}
