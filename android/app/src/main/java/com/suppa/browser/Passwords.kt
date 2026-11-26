package com.suppa.browser

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
import android.os.PersistableBundle
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.webkit.WebView
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Saved passwords (passwords.rs). Each password is encrypted with a key that
 * never leaves the phone's keystore. Showing, copying or exporting them asks
 * for the fingerprint, face or screen lock first (auth.rs).
 */
object Passwords {
    class Entry(val realm: String, var url: String, val user: String, var enc: String, var t: Long)

    private val list = mutableListOf<Entry>()
    private var loaded = false
    private var undo = mutableListOf<Entry>()
    private fun file() = File(U.dataDir(), "passwords.json")
    private fun neverFile() = File(U.dataDir(), "passwords_never.txt")
    private val never by lazy { (U.readText(neverFile()) ?: "").lines().filter { it.isNotBlank() }.toMutableSet() }

    // ---- encryption ---------------------------------------------------------------------

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey("suppa_passwords", null) as? SecretKey)?.let { return it }
        val g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        g.init(
            KeyGenParameterSpec.Builder("suppa_passwords", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return g.generateKey()
    }

    private fun encrypt(plain: String): String {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, key())
        val out = c.iv + c.doFinal(plain.toByteArray())
        return Base64.encodeToString(out, Base64.NO_WRAP)
    }

    private fun decrypt(enc: String): String? = try {
        val all = Base64.decode(enc, Base64.NO_WRAP)
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, all, 0, 12))
        String(c.doFinal(all, 12, all.size - 12))
    } catch (_: Exception) {
        null
    }

    // ---- storage ------------------------------------------------------------------------

    private fun items(): MutableList<Entry> {
        if (!loaded) {
            loaded = true
            val arr = try { JSONArray(U.readText(file()) ?: "[]") } catch (_: Exception) { JSONArray() }
            for (i in 0 until arr.length()) {
                val o = arr.optJSONObject(i) ?: continue
                list.add(Entry(o.optString("realm"), o.optString("url"), o.optString("user"), o.optString("enc"), o.optLong("t")))
            }
        }
        return list
    }

    private fun save() {
        val arr = JSONArray()
        items().forEach { arr.put(U.obj("realm" to it.realm, "url" to it.url, "user" to it.user, "enc" to it.enc, "t" to it.t)) }
        U.writeAtomic(file(), arr.toString())
    }

    fun forOrigin(origin: String): JSONArray {
        val out = JSONArray()
        items().filter { it.realm == origin }.sortedByDescending { it.t }.forEach { e ->
            decrypt(e.enc)?.let { out.put(U.obj("user" to e.user, "pass" to it)) }
        }
        return out
    }

    private fun put(realm: String, url: String, user: String, pass: String) {
        val e = items().firstOrNull { it.realm == realm && it.user == user }
        if (e != null) {
            e.enc = encrypt(pass)
            e.url = url
            e.t = System.currentTimeMillis()
        } else {
            items().add(Entry(realm, url, user, encrypt(pass), System.currentTimeMillis()))
        }
    }

    /** A sign-in form was sent: offer to save (or update) the password. */
    fun offerSave(origin: String, url: String, user: String, pass: String) {
        if (pass.isEmpty() || origin in never) return
        U.ui {
            val old = items().firstOrNull { it.realm == origin && it.user == user }
            if (old != null && decrypt(old.enc) == pass) return@ui
            val site = U.siteOf(origin)
            val title = if (old != null) "Update the saved password for $site?" else "Save the password for $site?"
            Popup.dialog(title, if (user.isNotEmpty()) "User name: $user" else "", listOf(if (old != null) "Update" else "Save", "Not now", "Never for this site"), center = true) { i ->
                when (i) {
                    0 -> { put(origin, url, user, pass); save() }
                    2 -> { never.add(origin); U.writeAtomic(neverFile(), never.joinToString("\n")) }
                }
            }
        }
    }

    // ---- the Passwords page of Settings ---------------------------------------------------

    fun listJson(): String {
        val arr = JSONArray()
        items().sortedBy { U.siteOf(it.realm) }.forEach {
            arr.put(U.obj("realm" to it.realm, "url" to it.url, "site" to U.siteOf(it.realm).ifEmpty { it.realm }, "user" to it.user, "bad" to false))
        }
        return U.obj("type" to "pw_list", "items" to arr, "pending" to 0).toString()
    }

    fun onPage(view: WebView, v: JSONObject) {
        val realm = v.optString("realm")
        val user = v.optString("user")
        val reply = { s: String -> Bridge.send(view, s) }
        val toast = { s: String -> reply(U.obj("type" to "pw_toast", "text" to s).toString()) }
        when (v.optString("op")) {
            "list" -> reply(listJson())
            "reveal", "copy" -> {
                val op = v.optString("op")
                Auth.require(if (op == "reveal") "Show a saved password" else "Copy a saved password") { ok ->
                    val e = items().firstOrNull { it.realm == realm && it.user == user }
                    val pass = e?.let { decrypt(it.enc) }
                    when {
                        !ok -> toast("Not confirmed - the password stays hidden")
                        op == "reveal" -> reply(U.obj("type" to "pw_reveal", "realm" to realm, "user" to user, "password" to pass).toString())
                        pass == null -> toast("Could not copy the password")
                        else -> {
                            val cm = U.ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                            val clip = ClipData.newPlainText("password", pass)
                            // Android 13+: kept out of the clipboard preview.
                            clip.description.extras = PersistableBundle().apply { putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true) }
                            cm.setPrimaryClip(clip)
                            toast("Password copied")
                        }
                    }
                }
            }
            "delete" -> {
                items().filter { it.realm == realm && it.user == user }.let { undo = it.toMutableList(); items().removeAll(it) }
                save()
                reply(listJson())
            }
            "undo" -> {
                items().addAll(undo)
                undo.clear()
                save()
                reply(listJson())
            }
        }
    }

    fun deleteAll() {
        items().clear()
        save()
    }

    fun clearAll() {
        Popup.confirm("Delete all saved passwords?", "This cannot be undone.", "Delete all") { deleteAll() }
    }

    // ---- .csv import / export (Chrome, Firefox, Edge, Brave format) ------------------------

    private fun parseCsv(text: String): List<List<String>> {
        val rows = mutableListOf<List<String>>()
        var row = mutableListOf<String>()
        val f = StringBuilder()
        var q = false
        var i = 0
        while (i < text.length) {
            val c = text[i]
            if (q) {
                if (c == '"') {
                    if (i + 1 < text.length && text[i + 1] == '"') { f.append('"'); i++ } else q = false
                } else f.append(c)
            } else when (c) {
                '"' -> q = true
                ',' -> { row.add(f.toString()); f.clear() }
                '\n', '\r' -> {
                    if (c == '\r' && i + 1 < text.length && text[i + 1] == '\n') i++
                    row.add(f.toString()); f.clear()
                    if (row.any { it.isNotEmpty() }) rows.add(row)
                    row = mutableListOf()
                }
                else -> f.append(c)
            }
            i++
        }
        row.add(f.toString())
        if (row.any { it.isNotEmpty() }) rows.add(row)
        return rows
    }

    fun importCsv() {
        MainActivity.act.openDocument(arrayOf("text/*", "application/csv", "text/comma-separated-values", "application/octet-stream")) { uri ->
            uri ?: return@openDocument
            val text = try { U.ctx.contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() } } catch (_: Exception) { null }
            val rows = parseCsv(text?.trimStart('﻿') ?: "")
            if (rows.size < 2) { Popup.message("Import passwords", "No passwords found in this file."); return@openDocument }
            val head = rows[0].map { it.trim().lowercase() }
            val iu = head.indexOfFirst { it == "url" || it == "origin" || it == "login_uri" }
            val iuser = head.indexOfFirst { it == "username" || it == "login_username" }
            val ip = head.indexOfFirst { it == "password" || it == "login_password" }
            if (iu < 0 || ip < 0) { Popup.message("Import passwords", "This is not a password export (it needs url and password columns)."); return@openDocument }
            var n = 0
            for (r in rows.drop(1)) {
                val url = r.getOrNull(iu)?.trim() ?: continue
                val pass = r.getOrNull(ip) ?: continue
                if (!url.startsWith("http") || pass.isEmpty()) continue
                val origin = try { android.net.Uri.parse(url).let { "${it.scheme}://${it.authority}" } } catch (_: Exception) { continue }
                put(origin, url, r.getOrNull(iuser) ?: "", pass)
                n++
            }
            save()
            Popup.message("Import passwords", "$n passwords imported. You can now delete the .csv file: it holds them as plain text.")
            PagesHost.broadcast()
        }
    }

    fun exportCsv() {
        Auth.require("Export saved passwords") { ok ->
            if (!ok) return@require
            MainActivity.act.createDocument("Suppa passwords.csv", "text/csv") { uri ->
                uri ?: return@createDocument
                fun q(s: String) = "\"" + s.replace("\"", "\"\"") + "\""
                val sb = StringBuilder("name,url,username,password\n")
                for (e in items()) sb.append(listOf(U.siteOf(e.realm), e.url.ifEmpty { e.realm }, e.user, decrypt(e.enc) ?: "").joinToString(",") { q(it) }).append('\n')
                U.ctx.contentResolver.openOutputStream(uri)?.use { it.write(sb.toString().toByteArray()) }
                Popup.message("Export passwords", "${items().size} passwords exported. The file holds them as plain text - keep it safe.")
            }
        }
    }
}

/** "Is it you?": fingerprint, face or the screen lock. One check is good for a minute. */
object Auth {
    private var okUntil = 0L

    fun require(reason: String, cb: (Boolean) -> Unit) {
        if (System.currentTimeMillis() < okUntil) { cb(true); return }
        val act = MainActivity.act
        val b = BiometricPrompt.Builder(act).setTitle("Suppa").setSubtitle(reason)
        if (Build.VERSION.SDK_INT >= 30) {
            b.setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_WEAK or BiometricManager.Authenticators.DEVICE_CREDENTIAL)
        } else {
            @Suppress("DEPRECATION")
            b.setDeviceCredentialAllowed(true)
        }
        // No screen lock at all: nothing to check against.
        val km = act.getSystemService(Context.KEYGUARD_SERVICE) as android.app.KeyguardManager
        if (!km.isDeviceSecure) { cb(true); return }
        try {
            b.build().authenticate(CancellationSignal(), act.mainExecutor, object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(r: BiometricPrompt.AuthenticationResult?) {
                    okUntil = System.currentTimeMillis() + 60_000
                    cb(true)
                }
                override fun onAuthenticationError(code: Int, msg: CharSequence?) = cb(false)
            })
        } catch (e: Exception) {
            U.log("auth: $e")
            cb(false)
        }
    }
}
