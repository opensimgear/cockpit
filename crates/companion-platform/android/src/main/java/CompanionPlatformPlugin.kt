package org.opensimgear.companion.platform

import android.app.Activity
import android.content.pm.PackageManager
import android.net.wifi.WifiManager
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class StoreArgs { lateinit var data: String }

@InvokeArg
class DiscoveryArgs { lateinit var token: String }

private const val LOCAL_NETWORK_PERMISSION = "android.permission.ACCESS_LOCAL_NETWORK"
private const val LOCAL_NETWORK_ALIAS = "localNetwork"
private const val LOCAL_NETWORK_API = 37

/** Cryptography with a non-exportable Android Keystore key; SQLite owns ciphertext.
 * Native Rust is the only caller; this plugin grants no webview commands.
 */
@TauriPlugin(permissions = [Permission(strings = [LOCAL_NETWORK_PERMISSION], alias = LOCAL_NETWORK_ALIAS)])
class CompanionPlatformPlugin(private val activity: Activity) : Plugin(activity) {
    private val alias = "org.opensimgear.companion.credential.v1"
    private val aad = "OpenSimGear companion credential v1".toByteArray(Charsets.UTF_8)
    private val maxBytes = 65536
    private val discovery = WifiDiscoveryLease(activity.applicationContext.getSystemService(WifiManager::class.java))
    // Permission requests and callbacks are owned by the Activity's UI thread.
    private var localNetworkRequest: Invoke? = null
    private val worker = ThreadPoolExecutor(1, 1, 0L, TimeUnit.MILLISECONDS,
        ArrayBlockingQueue<Runnable>(8), ThreadPoolExecutor.AbortPolicy())

    private fun key(create: Boolean): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val existing = store.getKey(alias, null)
        if (existing != null) return existing as SecretKey
        check(create) { "Credential key unavailable; forget this host and pair again" }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256).setRandomizedEncryptionRequired(true).build())
        }.generateKey()
    }
    private fun encrypt(plain: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key(true))
        cipher.updateAAD(aad)
        check(cipher.iv.size == 12)
        return byteArrayOf(1) + cipher.iv + cipher.doFinal(plain)
    }
    private fun decrypt(sealed: ByteArray): ByteArray {
        require(sealed.size in 30..(maxBytes + 29) && sealed[0] == 1.toByte())
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(false), GCMParameterSpec(128, sealed.copyOfRange(1, 13)))
        cipher.updateAAD(aad)
        return cipher.doFinal(sealed, 13, sealed.size - 13).also { require(it.size in 1..maxBytes) }
    }
    private fun execute(invoke: Invoke, operation: () -> JSObject) {
        try {
            worker.execute {
                try { invoke.resolve(operation()) }
                catch (_: Exception) { invoke.reject("Protected credential store unavailable or corrupt. Unlock the device, or Forget host and pair again.") }
            }
        } catch (_: Exception) { invoke.reject("Protected credential store is busy; retry after disconnecting.") }
    }
    @Command
    fun probe(invoke: Invoke) = execute(invoke) {
        val plain = byteArrayOf(42)
        val sealed = encrypt(plain)
        val opened = decrypt(sealed)
        check(opened.contentEquals(plain))
        plain.fill(0); opened.fill(0); sealed.fill(0)
        JSObject()
    }
    private fun transform(invoke: Invoke, sealing: Boolean) {
        val args = try { invoke.parseArgs(StoreArgs::class.java) }
            catch (_: Exception) { invoke.reject("Invalid native protection request"); return }
        execute(invoke) {
            val limit = if (sealing) maxBytes else maxBytes + 29
            require(args.data.length <= ((limit + 2) / 3) * 4)
            val input = Base64.decode(args.data, Base64.NO_WRAP)
            try {
                require(input.size in 1..limit)
                val output = if (sealing) encrypt(input) else decrypt(input)
                try { JSObject().put("data", Base64.encodeToString(output, Base64.NO_WRAP)) }
                finally { output.fill(0) }
            } finally { input.fill(0) }
        }
    }
    @Command
    fun seal(invoke: Invoke) = transform(invoke, true)
    @Command
    fun unseal(invoke: Invoke) = transform(invoke, false)

    private fun localNetworkAllowed(): Boolean =
        Build.VERSION.SDK_INT < LOCAL_NETWORK_API ||
            activity.applicationInfo.targetSdkVersion < LOCAL_NETWORK_API ||
            activity.checkSelfPermission(LOCAL_NETWORK_PERMISSION) == PackageManager.PERMISSION_GRANTED

    private fun localNetworkResult(prompted: Boolean): JSObject =
        JSObject().put("allowed", localNetworkAllowed()).put("prompted", prompted)

    @Command
    fun localNetworkStatus(invoke: Invoke) {
        invoke.resolve(localNetworkResult(false))
    }

    @Command
    fun requestLocalNetwork(invoke: Invoke) = activity.runOnUiThread {
        when {
            localNetworkAllowed() -> invoke.resolve(localNetworkResult(false))
            localNetworkRequest != null -> invoke.reject("A Local Network request is already open")
            activity.isFinishing || activity.isDestroyed || !activity.hasWindowFocus() ->
                invoke.reject("Open Companion before requesting Local Network access")
            else -> {
                localNetworkRequest = invoke
                try {
                    requestPermissionForAlias(LOCAL_NETWORK_ALIAS, invoke, "localNetworkPermissionResult")
                } catch (_: Exception) {
                    localNetworkRequest = null
                    invoke.reject("Android could not request Local Network access")
                }
            }
        }
    }

    @PermissionCallback
    fun localNetworkPermissionResult(invoke: Invoke) {
        if (localNetworkRequest !== invoke) return
        localNetworkRequest = null
        invoke.resolve(localNetworkResult(true))
    }

    @Command
    fun beginDiscovery(invoke: Invoke) {
        if (!localNetworkAllowed()) {
            invoke.reject("Allow Local Network access before starting discovery")
            return
        }
        try { invoke.resolve(JSObject().put("token", discovery.begin())) }
        catch (error: Exception) { invoke.reject(error.message ?: "Android Wi-Fi discovery is unavailable") }
    }

    @Command
    fun finishDiscovery(invoke: Invoke) {
        try {
            discovery.finish(invoke.parseArgs(DiscoveryArgs::class.java).token)
            invoke.resolve(JSObject())
        } catch (_: Exception) { invoke.reject("Could not finish Android Wi-Fi discovery") }
    }

    override fun onPause(activity: AppCompatActivity) = discovery.setForeground(false)
    override fun onResume(activity: AppCompatActivity) = discovery.setForeground(true)
    override fun onDestroy(activity: AppCompatActivity) {
        discovery.setForeground(false)
        val pending = localNetworkRequest
        localNetworkRequest = null
        pending?.reject("Companion closed before Local Network access was confirmed")
    }
}
