package com.aether.player

import android.content.Intent
import android.os.Build
import android.util.Log
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin

/**
 * Backend-facing control for TransferForegroundService, reached from the node
 * backend via reverse-RPC (callNative('transferServiceStart' | …) in
 * node-backend/transfer/server.ts). Mirrors MigrationServicePlugin. Also
 * exposes deviceName() so the transfer server can advertise/pair with a
 * human-readable phone name instead of a generic label.
 */
@CapacitorPlugin(name = "TransferService")
class TransferServicePlugin : Plugin() {

    private fun send(action: String?, done: Int, total: Int) {
        val intent = Intent(context, TransferForegroundService::class.java)
        if (action != null) intent.putExtra(TransferForegroundService.EXTRA_ACTION, action)
        intent.putExtra(TransferForegroundService.EXTRA_DONE, done)
        intent.putExtra(TransferForegroundService.EXTRA_TOTAL, total)
        // Best-effort: a progress update can arrive while the app is backgrounded,
        // where Android 12+ forbids (re)starting a FGS. The service is already
        // running from session start, so the notification just keeps its last
        // value — never fail the transfer over a missed update.
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        } catch (e: Exception) {
            Log.w("AETHER-TRANSFER", "FGS update skipped: ${e.message}")
        }
    }

    @PluginMethod
    fun start(call: PluginCall) {
        send(null, 0, 0)
        call.resolve()
    }

    @PluginMethod
    fun progress(call: PluginCall) {
        send(null, call.getInt("done") ?: 0, call.getInt("total") ?: 0)
        call.resolve()
    }

    @PluginMethod
    fun stop(call: PluginCall) {
        val intent = Intent(context, TransferForegroundService::class.java)
        intent.putExtra(TransferForegroundService.EXTRA_ACTION, TransferForegroundService.ACTION_STOP)
        try {
            context.startService(intent)
        } catch (_: Exception) {
            context.stopService(Intent(context, TransferForegroundService::class.java))
        }
        call.resolve()
    }

    @PluginMethod
    fun deviceName(call: PluginCall) {
        val manufacturer = Build.MANUFACTURER?.replaceFirstChar { it.uppercase() } ?: ""
        val model = Build.MODEL ?: ""
        val name = when {
            model.startsWith(manufacturer, ignoreCase = true) -> model
            manufacturer.isBlank() -> model
            else -> "$manufacturer $model"
        }.trim()
        call.resolve(JSObject().put("name", if (name.isBlank()) "Telefono Android" else name))
    }
}
