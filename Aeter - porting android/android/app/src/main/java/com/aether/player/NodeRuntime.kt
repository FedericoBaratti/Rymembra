package com.aether.player

import android.content.Context
import java.io.File

/**
 * Thin wrapper around the nodejs-mobile engine.
 *
 * Responsibilities:
 *   1. Copy the bundled node project (assets/nodejs-project) into a writable
 *      dir on first launch (the engine needs a real filesystem path).
 *   2. Start libnode on a background thread with that project's main.js.
 *   3. Bridge a single "message" channel both ways (JSON strings).
 *
 * The native methods below are provided by the nodejs-mobile prebuilt for
 * android-arm64 (libnode.so) plus a small JNI glue lib (libnode-bridge.so).
 * See MOBILE.md → "nodejs-mobile integration" for how to add them; they are
 * not committed here because they require the NDK to build/cross-compile.
 */
class NodeRuntime(private val context: Context) {

    /** Set by the plugin; receives JSON strings pushed from node. */
    var onMessage: ((String) -> Unit)? = null

    @Volatile private var engineStarted = false

    companion object {
        private const val PROJECT_ASSET_DIR = "nodejs-project"
        private const val CHANNEL = "aether"

        // Loaded from jniLibs/arm64-v8a (added during nodejs-mobile setup).
        init {
            System.loadLibrary("node")
            System.loadLibrary("node-bridge")
        }

        // Singleton so the JNI callback can route into the active runtime.
        @Volatile var active: NodeRuntime? = null

        /** Invoked from native code when node sends on the channel. */
        @JvmStatic
        fun receiveFromNode(channel: String, message: String) {
            if (channel == CHANNEL) active?.onMessage?.invoke(message)
        }
    }

    private external fun startNodeWithArguments(arguments: Array<String>): Int
    private external fun sendMessageToNode(channel: String, message: String)

    fun start() {
        if (engineStarted) return
        engineStarted = true
        active = this
        val projectDir = copyProjectIfNeeded()
        val mainJs = File(projectDir, "main.js").absolutePath
        Thread({ startNodeWithArguments(arrayOf("node", mainJs)) }, "nodejs-mobile").start()
    }

    fun send(message: String) {
        sendMessageToNode(CHANNEL, message)
    }

    /** Copies assets/nodejs-project → filesDir/nodejs-project (once per version). */
    private fun copyProjectIfNeeded(): File {
        val dest = File(context.filesDir, PROJECT_ASSET_DIR)
        val stamp = File(dest, ".copied-${appVersion()}")
        if (stamp.exists()) return dest
        if (dest.exists()) dest.deleteRecursively()
        copyAssetDir(PROJECT_ASSET_DIR, dest)
        stamp.parentFile?.mkdirs()
        stamp.createNewFile()
        return dest
    }

    private fun copyAssetDir(assetPath: String, dest: File) {
        val assets = context.assets
        val entries = assets.list(assetPath) ?: emptyArray()
        if (entries.isEmpty()) {
            // It's a file.
            dest.parentFile?.mkdirs()
            assets.open(assetPath).use { input -> dest.outputStream().use { input.copyTo(it) } }
            return
        }
        dest.mkdirs()
        for (entry in entries) copyAssetDir("$assetPath/$entry", File(dest, entry))
    }

    private fun appVersion(): String =
        try {
            context.packageManager.getPackageInfo(context.packageName, 0).versionName ?: "0"
        } catch (_: Exception) {
            "0"
        }
}
