package com.aether.player

import android.content.Intent
import android.os.Build
import android.util.Log
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin

/**
 * Backend-facing control for DownloadForegroundService. Reached from the
 * node-backend via reverse-RPC (callNative('downloadServiceStart' | 'Progress' |
 * 'Stop')): setDownloadLifecycle() in electron/modules/downloader.ts → nativeRpc.ts.
 *
 * start() launches the foreground service (valid only while foreground, where
 * downloads are initiated); progress()/stop() update the already-running
 * notification directly so they keep working while the app is backgrounded.
 */
@CapacitorPlugin(name = "DownloadNotification")
class DownloadNotificationPlugin : Plugin() {

    @PluginMethod
    fun start(call: PluginCall) {
        val intent = Intent(context, DownloadForegroundService::class.java)
        intent.putExtra(DownloadForegroundService.EXTRA_TITLE, call.getString("title") ?: "")
        intent.putExtra(DownloadForegroundService.EXTRA_TOTAL, call.getInt("total") ?: 0)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        } catch (e: Exception) {
            Log.w("AETHER-DOWNLOAD", "FGS start skipped: ${e.message}")
        }
        call.resolve()
    }

    @PluginMethod
    fun progress(call: PluginCall) {
        // Direct notify() — no startForegroundService, so it works in the background.
        DownloadForegroundService.applyProgress(
            context,
            call.getString("title") ?: "",
            call.getInt("active") ?: 0,
            call.getInt("total") ?: 0,
            call.getInt("percent") ?: 0
        )
        call.resolve()
    }

    @PluginMethod
    fun stop(call: PluginCall) {
        val intent = Intent(context, DownloadForegroundService::class.java)
        intent.putExtra(DownloadForegroundService.EXTRA_ACTION, DownloadForegroundService.ACTION_STOP)
        try {
            context.startService(intent)
        } catch (_: Exception) {
            context.stopService(Intent(context, DownloadForegroundService::class.java))
        }
        call.resolve()
    }
}
