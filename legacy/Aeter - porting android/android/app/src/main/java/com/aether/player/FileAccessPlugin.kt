package com.aether.player

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.Settings
import androidx.activity.result.ActivityResult
import androidx.browser.customtabs.CustomTabsIntent
import com.getcapacitor.JSObject
import com.getcapacitor.PermissionState
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.ActivityCallback
import com.getcapacitor.annotation.CapacitorPlugin
import com.getcapacitor.annotation.Permission
import com.getcapacitor.annotation.PermissionCallback
import androidx.documentfile.provider.DocumentFile
import java.io.File
import java.util.*
import java.util.concurrent.Executors
import org.json.JSONArray

/**
 * Folder selection (Storage Access Framework) and "open externally" intents.
 *
 * Replaces the Electron `dialog`/`shell` no-ops in node-backend/electron-shim.ts,
 * reached from the backend via the reverse-RPC layer (see callNative() and
 * src/lib/nativeRpc.ts).
 *   - pickFolder()       → ACTION_OPEN_DOCUMENT_TREE; persists the grant and
 *                          resolves the tree to a REAL filesystem path, because
 *                          the backend is path-based (parseFile, chokidar,
 *                          in-place tag writes) and cannot use content:// URIs.
 *   - showInFolder(path) → ACTION_VIEW on the containing folder.
 *   - openExternal(url)  → Custom Tab (Last.fm OAuth etc.).
 *
 * NOTE (device variance): mapping a tree URI to a path works for primary
 * external storage ("primary:..."); SD cards / USB OTG may not resolve to a
 * readable path. There the caller should fall back to SAF document I/O. The
 * READ_MEDIA_AUDIO runtime permission still governs reading the audio files.
 */
@CapacitorPlugin(
    name = "FileAccess",
    permissions = [
        Permission(strings = [Manifest.permission.READ_MEDIA_AUDIO], alias = "audio"),
        Permission(strings = [Manifest.permission.READ_EXTERNAL_STORAGE], alias = "storage")
    ]
)
class FileAccessPlugin : Plugin() {

    // Shared worker pool for the blocking I/O methods (scanFolder, saveFileViaSaf,
    // deleteFile). Cached pool: preserves their original run-in-parallel semantics
    // (a long SAF scan must not delay a save/delete) while giving the plugin a
    // single handle to release on destroy.
    private val executor = Executors.newCachedThreadPool()

    override fun handleOnDestroy() {
        executor.shutdownNow()
        super.handleOnDestroy()
    }

    @PluginMethod
    fun pickFolder(call: PluginCall) {
        // The backend reads audio by REAL file path (parseFile/chokidar), which on
        // Android 13+ needs the READ_MEDIA_AUDIO runtime grant — the SAF tree grant
        // alone only covers content:// I/O. Ask for it now so the prompt appears
        // exactly when the user adds a folder. (Device variance: pre-13 devices use
        // READ_EXTERNAL_STORAGE; minSdk 26 — see manifest note for legacy coverage.)
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            if (getPermissionState("audio") != PermissionState.GRANTED) {
                requestPermissionForAlias("audio", call, "afterPerm")
                return
            }
        } else {
            if (getPermissionState("storage") != PermissionState.GRANTED) {
                requestPermissionForAlias("storage", call, "afterPerm")
                return
            }
        }
        launchTreePicker(call)
    }

    @PermissionCallback
    private fun afterPerm(call: PluginCall) {
        // Open the picker regardless of the grant outcome: the user may target
        // app-local storage, or grant audio later from system Settings.
        launchTreePicker(call)
    }

    private fun launchTreePicker(call: PluginCall) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
        startActivityForResult(call, intent, "onFolderPicked")
    }

    @ActivityCallback
    private fun onFolderPicked(call: PluginCall, result: ActivityResult) {
        val data = result.data
        val treeUri = data?.data
        if (treeUri == null) {
            call.resolve(JSObject().put("canceled", true))
            return
        }
        // Keep the grant across restarts.
        val flags =
            Intent.FLAG_GRANT_READ_URI_PERMISSION or
            Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        try {
            context.contentResolver.takePersistableUriPermission(treeUri, flags)
        } catch (_: SecurityException) {
            /* some providers don't allow persisting; reading may still work this session */
        }
        val path = treeUriToPath(treeUri)
        if (path == null) {
            call.reject("unresolved-path")
            return
        }
        call.resolve(JSObject().put("canceled", false).put("path", path))
    }

    /**
     * All Files Access (MANAGE_EXTERNAL_STORAGE) state. The auto-setup at app
     * startup (src/lib/androidStorage.ts) needs broad storage access to CREATE
     * an empty `Download/Music` folder and to read/write it by path: scoped
     * storage (READ_MEDIA_AUDIO / SAF) cannot create an empty, persistent folder
     * in a public collection. Pre-Android 11 (API 30) the concept doesn't exist;
     * we report granted there (legacy storage model still applies).
     */
    @PluginMethod
    fun hasAllFilesAccess(call: PluginCall) {
        call.resolve(JSObject().put("granted", isAllFilesManager()))
    }

    /**
     * Reports the active connection transport so the backend can honour the
     * "download missing tracks only on Wi-Fi" setting while headless in
     * nodejs-mobile. Returns 'wifi' | 'cellular' | 'ethernet' | 'none'. Wi-Fi
     * and Ethernet are unmetered-by-default; cellular is the metered case the
     * gate blocks. Any error resolves to 'none' (caller treats it as allowed).
     */
    @PluginMethod
    fun getNetworkType(call: PluginCall) {
        val type = try {
            val cm = context.getSystemService(android.content.Context.CONNECTIVITY_SERVICE)
                as android.net.ConnectivityManager
            val network = cm.activeNetwork
            val caps = if (network != null) cm.getNetworkCapabilities(network) else null
            when {
                caps == null -> "none"
                caps.hasTransport(android.net.NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
                caps.hasTransport(android.net.NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
                caps.hasTransport(android.net.NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
                else -> "none"
            }
        } catch (_: Exception) {
            "none"
        }
        call.resolve(JSObject().put("type", type))
    }

    /**
     * Sends the user to the system "All files access" screen for this app, then
     * re-reads the grant when they return. ACTION_MANAGE_APP_ALL_FILES_ACCESS_
     * PERMISSION never delivers a meaningful resultCode, so we ignore it and just
     * re-check isExternalStorageManager() in the activity callback.
     */
    @PluginMethod
    fun requestAllFilesAccess(call: PluginCall) {
        if (isAllFilesManager()) {
            call.resolve(JSObject().put("granted", true))
            return
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            // No All-Files-Access concept pre-API 30; nothing to request.
            call.resolve(JSObject().put("granted", true))
            return
        }
        val intent = try {
            Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:" + context.packageName)
            )
        } catch (_: Exception) {
            // Some OEMs lack the per-app screen; fall back to the global list.
            Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION)
        }
        startActivityForResult(call, intent, "afterManageRequest")
    }

    @ActivityCallback
    private fun afterManageRequest(call: PluginCall, result: ActivityResult) {
        call.resolve(JSObject().put("granted", isAllFilesManager()))
    }

    /**
     * Ensures `<primary external>/Download/Music` exists and returns its real
     * path. Creating an empty folder in a public collection only works with
     * All Files Access (best-effort mkdirs(): succeeds when granted, returns
     * false otherwise without throwing). The path is ALWAYS returned so the
     * caller can persist it even when the folder couldn't be created yet — it
     * will be created on a later launch once access is granted.
     */
    @PluginMethod
    fun ensureDownloadMusicFolder(call: PluginCall) {
        val downloads = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
        val dir = File(downloads, "Music")
        val existed = dir.isDirectory
        if (!existed) {
            try {
                dir.mkdirs()
            } catch (_: Exception) {
                /* no access yet; path still returned */
            }
        }
        call.resolve(
            JSObject()
                .put("path", dir.absolutePath)
                .put("existed", existed)
        )
    }

    private fun isAllFilesManager(): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Environment.isExternalStorageManager()
        } else {
            true
        }
    }

    @PluginMethod
    fun showInFolder(call: PluginCall) {
        val path = call.getString("path")
        if (path == null) {
            call.reject("path is required")
            return
        }
        val file = File(path)
        val folder = if (file.isDirectory) file else file.parentFile ?: file
        val uri = Uri.fromFile(folder)
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "resource/folder")
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        try {
            context.startActivity(intent)
        } catch (_: Exception) {
            /* no file manager handles resource/folder; nothing to do */
        }
        call.resolve()
    }

    @PluginMethod
    fun openExternal(call: PluginCall) {
        val url = call.getString("url")
        if (url == null) {
            call.reject("url is required")
            return
        }
        try {
            val tabs = CustomTabsIntent.Builder().build()
            tabs.intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            tabs.launchUrl(context, Uri.parse(url))
        } catch (_: Exception) {
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(intent)
        }
        call.resolve()
    }

    @PluginMethod
    fun scanFolder(call: PluginCall) {
        val uriString = call.getString("uri") ?: return call.reject("URI is required")
        val treeUri = Uri.parse(uriString)

        // Launch on a background thread to avoid blocking the bridge/UI
        executor.execute {
            try {
                val results = mutableListOf<JSObject>()
                val rootDocFile = DocumentFile.fromTreeUri(context, treeUri)
                if (rootDocFile == null || !rootDocFile.isDirectory) {
                    call.reject("Invalid directory URI")
                    return@execute
                }

                // Iterative traversal to avoid StackOverflow on deep trees
                val stack = ArrayDeque<DocumentFile>()
                stack.push(rootDocFile)

                while (stack.isNotEmpty()) {
                    val current = stack.pop()
                    if (current.isDirectory) {
                        val children = current.listFiles()
                        for (child in children) {
                            if (child != null && !child.name.isNullOrEmpty()) {
                                stack.push(child)
                            }
                        }
                    } else if (!current.name.isNullOrBlank()) {
                        val fileObj = JSObject()
                        fileObj.put("name", current.name)
                        fileObj.put("size", current.length())
                        fileObj.put("mimeType", current.type ?: "application/octet-stream")
                        fileObj.put("uri", current.uri.toString())
                        results.add(fileObj)
                    }
                }

                val response = JSObject()
                response.put("files", JSONArray(results))
                call.resolve(response)
            } catch (e: Exception) {
                call.reject(e.message ?: "Unknown error during scan")
            }
        }
    }

    /**
     * Writes a (locally-edited) temp file back over an original on shared storage
     * via SAF, using the persisted tree grant from pickFolder. The Node backend
     * can read the original by path (READ_MEDIA_AUDIO) but cannot WRITE it
     * (scoped storage), so it edits a private temp copy and asks us to copy it
     * back here. Reached over the reverse-RPC layer (see src/lib/nativeRpc.ts).
     */
    @PluginMethod
    fun saveFileViaSaf(call: PluginCall) {
        val originalPath = call.getString("originalPath")
            ?: return call.reject("originalPath is required")
        val tempPath = call.getString("tempPath")
            ?: return call.reject("tempPath is required")
        executor.execute {
            try {
                val docUri = resolveDocUriForPath(originalPath)
                if (docUri == null) {
                    // No SAF tree grant covers this path (e.g. the auto-setup
                    // Download/Music folder, added via MANAGE_EXTERNAL_STORAGE
                    // rather than pickFolder). With All Files Access we can write
                    // the original by path directly; otherwise there's no way in.
                    if (isAllFilesManager()) {
                        File(tempPath).inputStream().use { src ->
                            File(originalPath).outputStream().use { sink -> src.copyTo(sink) }
                        }
                        call.resolve(JSObject().put("ok", true))
                        return@execute
                    }
                    call.reject("no-saf-grant")
                    return@execute
                }
                // "wt" = write + truncate the existing document in place, keeping
                // its identity (same file / MediaStore entry).
                val out = context.contentResolver.openOutputStream(docUri, "wt")
                if (out == null) {
                    call.reject("open-output-failed")
                    return@execute
                }
                out.use { sink -> File(tempPath).inputStream().use { it.copyTo(sink) } }
                call.resolve(JSObject().put("ok", true))
            } catch (e: Exception) {
                call.reject(e.message ?: "saf-write-failed")
            }
        }
    }

    /**
     * Creates a NEW document `fileName` inside `folderPath` on shared storage
     * from a private temp file. Companion to saveFileViaSaf for the phone-repair
     * commit when the file EXTENSION changes (.opus → .mp3): SAF cannot rename
     * a document's extension in place, so the new file is created and the old
     * one dropped by the caller. Same dual path as saveFileViaSaf: direct fs
     * copy under All Files Access, SAF createDocument under a tree grant.
     */
    @PluginMethod
    fun importFileViaSaf(call: PluginCall) {
        val folderPath = call.getString("folderPath")
            ?: return call.reject("folderPath is required")
        val fileName = call.getString("fileName")
            ?: return call.reject("fileName is required")
        val tempPath = call.getString("tempPath")
            ?: return call.reject("tempPath is required")
        if (fileName.contains('/') || fileName.contains('\\')) {
            return call.reject("bad-file-name")
        }
        executor.execute {
            try {
                val destFile = File(folderPath, fileName)
                if (isAllFilesManager()) {
                    File(tempPath).inputStream().use { src ->
                        destFile.outputStream().use { sink -> src.copyTo(sink) }
                    }
                    call.resolve(JSObject().put("ok", true).put("path", destFile.absolutePath))
                    return@execute
                }
                val folderUri = resolveDocUriForPath(folderPath)
                if (folderUri == null) {
                    call.reject("no-saf-grant")
                    return@execute
                }
                val mime = when (fileName.substringAfterLast('.', "").lowercase()) {
                    "mp3" -> "audio/mpeg"
                    "flac" -> "audio/flac"
                    "m4a", "mp4" -> "audio/mp4"
                    "aac" -> "audio/aac"
                    "ogg", "opus" -> "audio/ogg"
                    "wav" -> "audio/wav"
                    else -> "application/octet-stream"
                }
                val docUri = android.provider.DocumentsContract.createDocument(
                    context.contentResolver,
                    folderUri,
                    mime,
                    fileName
                )
                if (docUri == null) {
                    call.reject("create-document-failed")
                    return@execute
                }
                val out = context.contentResolver.openOutputStream(docUri, "wt")
                if (out == null) {
                    call.reject("open-output-failed")
                    return@execute
                }
                out.use { sink -> File(tempPath).inputStream().use { it.copyTo(sink) } }
                call.resolve(JSObject().put("ok", true).put("path", destFile.absolutePath))
            } catch (e: Exception) {
                call.reject(e.message ?: "saf-import-failed")
            }
        }
    }

    /**
     * Permanently deletes a file on shared storage. Mirrors saveFileViaSaf's
     * dual path: a SAF document delete when a persisted WRITE tree grant covers
     * the path, otherwise a direct File.delete() when All Files Access is
     * granted. Android has no Recycle Bin, so this is irreversible — the backend
     * only calls it for confirmed duplicate losers (electron-shim trashItem).
     * A file that's already gone counts as success (idempotent for rescans).
     */
    @PluginMethod
    fun deleteFile(call: PluginCall) {
        val path = call.getString("path")
            ?: return call.reject("path is required")
        executor.execute {
            try {
                val file = File(path)
                if (!file.exists()) {
                    call.resolve(JSObject().put("ok", true))
                    return@execute
                }
                val docUri = resolveDocUriForPath(path)
                if (docUri != null) {
                    val ok = android.provider.DocumentsContract.deleteDocument(
                        context.contentResolver,
                        docUri
                    )
                    if (ok) {
                        call.resolve(JSObject().put("ok", true))
                    } else {
                        call.reject("saf-delete-failed")
                    }
                    return@execute
                }
                // No SAF tree grant covers this path (e.g. the auto-setup
                // Download/Music folder added via MANAGE_EXTERNAL_STORAGE). With
                // All Files Access we can delete by path directly.
                if (isAllFilesManager()) {
                    if (file.delete()) {
                        call.resolve(JSObject().put("ok", true))
                    } else {
                        call.reject("delete-failed")
                    }
                    return@execute
                }
                call.reject("no-saf-grant")
            } catch (e: Exception) {
                call.reject(e.message ?: "delete-failed")
            }
        }
    }

    /**
     * Full process restart via the RestartActivity trampoline (own process):
     * the only recovery when the nodejs-mobile engine is dead, since it cannot
     * be restarted in-process. Resolves before handing off so the WebView gets
     * the ack; the kill follows within milliseconds.
     */
    @PluginMethod
    fun restartApp(call: PluginCall) {
        val ctx = context.applicationContext
        val intent = Intent(ctx, RestartActivity::class.java)
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        intent.putExtra(RestartActivity.EXTRA_MAIN_PID, android.os.Process.myPid())
        call.resolve()
        ctx.startActivity(intent)
    }

    /**
     * Resolves a real filesystem path under primary external storage to its SAF
     * document URI, using a persisted WRITE tree grant whose folder contains it.
     * Returns null when no granted tree covers the path (e.g. removable volumes).
     */
    private fun resolveDocUriForPath(path: String): Uri? {
        val base = Environment.getExternalStorageDirectory().absolutePath
        if (!path.startsWith("$base/")) return null
        for (perm in context.contentResolver.persistedUriPermissions) {
            if (!perm.isWritePermission) continue
            val treePath = treeUriToPath(perm.uri) ?: continue
            if (path == treePath || path.startsWith("$treePath/")) {
                val fileDocId = "primary:" + path.removePrefix("$base/")
                return android.provider.DocumentsContract.buildDocumentUriUsingTree(
                    perm.uri,
                    fileDocId
                )
            }
        }
        return null
    }

    /**
     * Best-effort SAF tree URI → real path. Handles the primary-storage form
     * `.../tree/primary:Sub/Dir`. Returns null for providers we can't map
     * (e.g. removable volumes).
     */
    private fun treeUriToPath(treeUri: Uri): String? {
        val docId = android.provider.DocumentsContract.getTreeDocumentId(treeUri)
        val parts = docId.split(":", limit = 2)
        if (parts.size != 2) return null
        val (volume, relative) = parts
        if (volume != "primary") return null
        val base = Environment.getExternalStorageDirectory().absolutePath
        return if (relative.isEmpty()) base else "$base/$relative"
    }
}
