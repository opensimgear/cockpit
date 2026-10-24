package org.opensimgear.companion.platform

import android.app.Activity
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
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

/** Cryptography with a non-exportable Android Keystore key; SQLite owns ciphertext.
 * Native Rust is the only caller; this plugin grants no webview commands.
 */
@TauriPlugin
class CompanionPlatformPlugin(private val activity: Activity) : Plugin(activity) {
    private val alias = "org.opensimgear.companion.credential.v1"
    private val aad = "OpenSimGear companion credential v1".toByteArray(Charsets.UTF_8)
    private val maxBytes = 65536
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
}
