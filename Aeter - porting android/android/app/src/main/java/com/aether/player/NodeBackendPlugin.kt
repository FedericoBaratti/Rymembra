package com.aether.player

import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import org.json.JSONObject

/**
 * Renderer-facing Capacitor plugin that owns the nodejs-mobile runtime.
 *
 * Contract with src/lib/bridge.ts:
 *   - start()                  → ensures the node engine is running
 *   - send({ message })        → forwards a JSON string to the node process
 *   - addListener("message")   → receives JSON strings pushed from node
 *
 * On first start it sends an `init` message carrying the Android directories
 * (HostConfig in node-backend/runtime.ts) so the backend can place the DB,
 * settings and downloads, and locate the ARM lib*.so binaries.
 */
@CapacitorPlugin(name = "NodeBackend")
class NodeBackendPlugin : Plugin() {

    private val runtime by lazy { NodeRuntime(context) }
    @Volatile private var started = false

    override fun load() {
        // Stream node → renderer messages out as the "message" plugin event.
        runtime.onMessage = { msg ->
            val data = JSObject()
            data.put("message", msg)
            notifyListeners("message", data)
        }
    }

    @PluginMethod
    fun start(call: PluginCall) {
        if (!started) {
            started = true
            runtime.start()
            runtime.send(buildInitMessage().toString())
        }
        call.resolve()
    }

    @PluginMethod
    fun send(call: PluginCall) {
        val message = call.getString("message")
        if (message == null) {
            call.reject("message is required")
            return
        }
        runtime.send(message)
        call.resolve()
    }

    /** Android directories handed to the node backend (see runtime.ts HostConfig). */
    private fun buildInitMessage(): JSONObject {
        val host = JSONObject().apply {
            put("dataDir", context.filesDir.absolutePath)
            put("filesDir", context.filesDir.absolutePath)
            put("cacheDir", context.cacheDir.absolutePath)
            put("musicDir", context.getExternalFilesDir("Music")?.absolutePath ?: context.filesDir.absolutePath)
            put("nativeLibraryDir", context.applicationInfo.nativeLibraryDir)
        }
        return JSONObject().apply {
            put("t", "init")
            put("host", host)
        }
    }
}
