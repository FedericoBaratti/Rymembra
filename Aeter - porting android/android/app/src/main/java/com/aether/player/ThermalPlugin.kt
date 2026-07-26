package com.aether.player

import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin

/**
 * Bridges [ThermalMonitor] to the WebView. src/lib/thermal.ts subscribes to
 * `thermalchanged` and forwards each sample to the node backend through the
 * `thermalUpdate` IPC, where adaptiveConcurrency.ts retunes the scan and
 * enrichment queues. Emits only on level transitions (a handful per session),
 * so the bridge cost is negligible.
 */
@CapacitorPlugin(name = "Thermal")
class ThermalPlugin : Plugin() {

    private var unsubscribe: (() -> Unit)? = null

    override fun load() {
        ThermalMonitor.start(context)
        unsubscribe = ThermalMonitor.addListener { level ->
            notifyListeners("thermalchanged", stateJs(level))
        }
    }

    @PluginMethod
    fun getState(call: PluginCall) {
        call.resolve(stateJs(ThermalMonitor.currentLevel))
    }

    private fun stateJs(level: String): JSObject {
        val data = JSObject()
            .put("level", level)
            .put("timestamp", System.currentTimeMillis())
        ThermalMonitor.headroom()?.let { data.put("headroom", it.toDouble()) }
        return data
    }

    override fun handleOnDestroy() {
        unsubscribe?.invoke()
        unsubscribe = null
    }
}
