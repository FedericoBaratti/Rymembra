package com.aether.player

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin

/**
 * Browses for the desktop's LAN server via mDNS/NSD (`_aether._tcp`),
 * advertised by electron/modules/lan/mdns.ts on the desktop. Used by
 * src/lib/lanClient.ts to re-find the paired desktop when its IP has changed
 * since the last session (DHCP lease renewal, hotspot reconnect, ...) —
 * pairing itself (PairDevice.tsx's QR scan) already gives an initial
 * host/port directly, so this is a resilience fallback, not the primary path.
 *
 * Emits `serviceFound` events with {name, host, port} as each advertised
 * instance resolves.
 *
 * Also ADVERTISES the phone's own transfer server (`_aether-transfer._tcp`,
 * node-backend/transfer/server.ts) via registerService/unregisterService, so
 * the paired desktop can re-find the phone after a DHCP/IP change. TXT records
 * carry {deviceId, deviceName} for an identity match before any auth attempt.
 */
@CapacitorPlugin(name = "LanDiscovery")
class LanDiscoveryPlugin : Plugin() {

    private var nsdManager: NsdManager? = null
    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private var multicastLock: WifiManager.MulticastLock? = null
    private var registrationListener: NsdManager.RegistrationListener? = null

    @PluginMethod
    fun startDiscovery(call: PluginCall) {
        if (discoveryListener != null) {
            call.resolve()
            return
        }
        try {
            val manager = context.getSystemService(Context.NSD_SERVICE) as NsdManager
            nsdManager = manager

            // Many devices drop multicast packets by default to save power; without
            // this lock mDNS responses silently never arrive.
            val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
            multicastLock = wifi.createMulticastLock("aether-lan-discovery").apply {
                setReferenceCounted(true)
                acquire()
            }

            val listener = object : NsdManager.DiscoveryListener {
                override fun onDiscoveryStarted(serviceType: String) {}
                override fun onDiscoveryStopped(serviceType: String) {}
                override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                    notifyListeners("discoveryError", JSObject().put("code", errorCode))
                }
                override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}
                override fun onServiceFound(service: NsdServiceInfo) {
                    if (!service.serviceType.contains("_aether")) return
                    resolveService(service)
                }
                override fun onServiceLost(service: NsdServiceInfo) {
                    notifyListeners("serviceLost", JSObject().put("name", service.serviceName))
                }
            }
            discoveryListener = listener
            manager.discoverServices("_aether._tcp.", NsdManager.PROTOCOL_DNS_SD, listener)
            call.resolve()
        } catch (e: Exception) {
            call.reject("LAN discovery start error: ${e.message}", e)
        }
    }

    private fun resolveService(service: NsdServiceInfo) {
        val manager = nsdManager ?: return
        manager.resolveService(service, object : NsdManager.ResolveListener {
            override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {}
            override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                val data = JSObject()
                    .put("name", serviceInfo.serviceName)
                    .put("host", serviceInfo.host?.hostAddress)
                    .put("port", serviceInfo.port)
                notifyListeners("serviceFound", data)
            }
        })
    }

    @PluginMethod
    fun stopDiscovery(call: PluginCall) {
        stopInternal()
        call.resolve()
    }

    /**
     * Advertises the phone transfer server on the LAN. Idempotent: a second
     * call (e.g. server restart on a new port) re-registers with fresh info.
     */
    @PluginMethod
    fun registerService(call: PluginCall) {
        val port = call.getInt("port") ?: return call.reject("port is required")
        val deviceId = call.getString("deviceId") ?: ""
        val deviceName = call.getString("deviceName") ?: "Telefono Android"
        try {
            val manager = nsdManager
                ?: (context.getSystemService(Context.NSD_SERVICE) as NsdManager).also { nsdManager = it }
            unregisterInternal(manager)

            val info = NsdServiceInfo().apply {
                serviceName = "Aether $deviceName"
                serviceType = "_aether-transfer._tcp."
                setPort(port)
                setAttribute("deviceId", deviceId)
                setAttribute("deviceName", deviceName)
            }
            val listener = object : NsdManager.RegistrationListener {
                override fun onServiceRegistered(serviceInfo: NsdServiceInfo) {}
                override fun onRegistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                    notifyListeners("registerError", JSObject().put("code", errorCode))
                }
                override fun onServiceUnregistered(serviceInfo: NsdServiceInfo) {}
                override fun onUnregistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {}
            }
            registrationListener = listener
            manager.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
            call.resolve()
        } catch (e: Exception) {
            call.reject("NSD register error: ${e.message}", e)
        }
    }

    @PluginMethod
    fun unregisterService(call: PluginCall) {
        try {
            nsdManager?.let { unregisterInternal(it) }
        } catch (_: Exception) {
            /* best effort */
        }
        call.resolve()
    }

    private fun unregisterInternal(manager: NsdManager) {
        registrationListener?.let {
            try {
                manager.unregisterService(it)
            } catch (_: Exception) {
                /* not registered */
            }
        }
        registrationListener = null
    }

    private fun stopInternal() {
        try {
            discoveryListener?.let { nsdManager?.stopServiceDiscovery(it) }
        } catch (_: Exception) {
            // already stopped/never started
        }
        discoveryListener = null
        try {
            multicastLock?.let { if (it.isHeld) it.release() }
        } catch (_: Exception) {
            /* best effort */
        }
        multicastLock = null
    }

    override fun handleOnDestroy() {
        stopInternal()
        try {
            nsdManager?.let { unregisterInternal(it) }
        } catch (_: Exception) {
            /* best effort */
        }
    }
}
