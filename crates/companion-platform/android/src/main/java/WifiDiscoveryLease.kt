package org.opensimgear.companion.platform

import android.net.wifi.WifiManager
import android.os.Handler
import android.os.Looper
import java.util.UUID

internal class WifiDiscoveryLease(private val wifi: WifiManager?) {
    private data class Lease(val token: String, val lock: WifiManager.MulticastLock?, val expiry: Runnable)

    private val handler = Handler(Looper.getMainLooper())
    private var foreground = true
    private var current: Lease? = null

    @Synchronized
    fun begin(): String {
        check(foreground) { "Open Companion before starting discovery" }
        check(current == null) { "Companion discovery is already running" }
        val token = UUID.randomUUID().toString()
        val lock = wifi?.createMulticastLock("opensimgear-companion-discovery")
        val expiry = Runnable { finish(token) }
        current = Lease(token, lock, expiry)
        try {
            lock?.setReferenceCounted(false)
            lock?.acquire()
            // Rust discovery is capped at three seconds. Retire an abandoned bridge call.
            check(handler.postDelayed(expiry, 4_000L))
            return token
        } catch (error: Exception) {
            finish(token)
            throw error
        }
    }

    @Synchronized
    fun finish(token: String) {
        val lease = current?.takeIf { it.token == token } ?: return
        current = null
        handler.removeCallbacks(lease.expiry)
        lease.lock?.takeIf { it.isHeld }?.release()
    }

    @Synchronized
    fun setForeground(active: Boolean) {
        foreground = active
        if (!active) current?.let { finish(it.token) }
    }
}
