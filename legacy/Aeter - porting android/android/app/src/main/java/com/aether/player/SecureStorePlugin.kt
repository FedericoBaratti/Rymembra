package com.aether.player

import android.util.Base64
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import java.security.SecureRandom

/**
 * Provides a stable 32-byte AES key for the node-backend's at-rest secret
 * encryption (see safeStorage in node-backend/electron-shim.ts). The key is
 * generated once and stored in EncryptedSharedPreferences (Android Keystore
 * master key), then handed to Node via reverse-RPC (callNative('getSecureKey')).
 *
 * Also exposes generic getValue/setValue/deleteValue on the same encrypted
 * store, used directly by the renderer (not reverse-RPC — there is no Node
 * backend in LAN thin-client mode) to persist the desktop pairing: host,
 * port and deviceToken (src/lib/lanClient.ts).
 *
 * Requires the dependency androidx.security:security-crypto in app build.gradle.
 */
@CapacitorPlugin(name = "SecureStore")
class SecureStorePlugin : Plugin() {

    private val prefs by lazy {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        EncryptedSharedPreferences.create(
            context,
            "aether_secure_store",
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
    }

    @PluginMethod
    fun getKey(call: PluginCall) {
        try {
            var keyB64 = prefs.getString(KEY_PREF, null)
            if (keyB64 == null) {
                val raw = ByteArray(32)
                SecureRandom().nextBytes(raw)
                keyB64 = Base64.encodeToString(raw, Base64.NO_WRAP)
                prefs.edit().putString(KEY_PREF, keyB64).apply()
            }
            call.resolve(JSObject().put("keyB64", keyB64))
        } catch (e: Exception) {
            call.reject("secure key error: ${e.message}", e)
        }
    }

    @PluginMethod
    fun getValue(call: PluginCall) {
        val key = call.getString("key")
        if (key == null) {
            call.reject("key is required")
            return
        }
        try {
            call.resolve(JSObject().put("value", prefs.getString(key, null)))
        } catch (e: Exception) {
            call.reject("secure store read error: ${e.message}", e)
        }
    }

    @PluginMethod
    fun setValue(call: PluginCall) {
        val key = call.getString("key")
        val value = call.getString("value")
        if (key == null || value == null) {
            call.reject("key and value are required")
            return
        }
        try {
            prefs.edit().putString(key, value).apply()
            call.resolve()
        } catch (e: Exception) {
            call.reject("secure store write error: ${e.message}", e)
        }
    }

    @PluginMethod
    fun deleteValue(call: PluginCall) {
        val key = call.getString("key")
        if (key == null) {
            call.reject("key is required")
            return
        }
        try {
            prefs.edit().remove(key).apply()
            call.resolve()
        } catch (e: Exception) {
            call.reject("secure store delete error: ${e.message}", e)
        }
    }

    companion object {
        private const val KEY_PREF = "aes_key_v1"
    }
}
