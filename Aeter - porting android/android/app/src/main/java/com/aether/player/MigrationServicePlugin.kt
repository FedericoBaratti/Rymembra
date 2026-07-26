package com.aether.player

import android.content.Intent
import android.os.Build
import android.util.Log
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin

/**
 * Renderer/backend-facing control for MigrationForegroundService. Reached from
 * the node-backend via reverse-RPC (callNative('migrationServiceStart' | …)):
 * setMigrationLifecycle() in electron/modules/spotifyMigration.ts → nativeRpc.ts.
 *
 * Keeping the process alive during the migration is what lets the (concurrent)
 * yt-dlp downloads keep running while the app is backgrounded.
 */
@CapacitorPlugin(name = "MigrationService")
class MigrationServicePlugin : Plugin() {

    private fun send(action: String?, done: Int, total: Int) {
        val intent = Intent(context, MigrationForegroundService::class.java)
        if (action != null) intent.putExtra(MigrationForegroundService.EXTRA_ACTION, action)
        intent.putExtra(MigrationForegroundService.EXTRA_DONE, done)
        intent.putExtra(MigrationForegroundService.EXTRA_TOTAL, total)
        // Best-effort: a progress update can arrive while the app is backgrounded,
        // where Android 12+ forbids (re)starting a FGS. The service is already
        // running from migration start, so the visible notification simply keeps
        // its last value — never crash the migration over a missed update.
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        } catch (e: Exception) {
            Log.w("AETHER-MIGRATION", "FGS update skipped: ${e.message}")
        }
    }

    @PluginMethod
    fun start(call: PluginCall) {
        send(null, 0, call.getInt("total") ?: 0)
        call.resolve()
    }

    @PluginMethod
    fun progress(call: PluginCall) {
        send(null, call.getInt("done") ?: 0, call.getInt("total") ?: 0)
        call.resolve()
    }

    @PluginMethod
    fun stop(call: PluginCall) {
        // A plain stopService is enough, but route through onStartCommand so the
        // service tears down its own foreground state cleanly.
        val intent = Intent(context, MigrationForegroundService::class.java)
        intent.putExtra(MigrationForegroundService.EXTRA_ACTION, MigrationForegroundService.ACTION_STOP)
        try {
            context.startService(intent)
        } catch (_: Exception) {
            context.stopService(Intent(context, MigrationForegroundService::class.java))
        }
        call.resolve()
    }
}
