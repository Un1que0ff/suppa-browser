package com.suppa.browser

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * Updates (updater.rs): once a day Suppa asks GitHub for the newest release
 * (only the version number is read). A release with Suppa-<version>-android.apk
 * can be installed with one tap; Android asks to confirm the install.
 */
object Updater {
    private const val API = "https://api.github.com/repos/Un1que0ff/suppa-browser/releases"
    private const val PAGE = "https://github.com/Un1que0ff/suppa-browser/releases"
    private fun stampFile() = File(U.dataDir(), "update_check.txt")
    private fun lastRunFile() = File(U.dataDir(), "last_version.txt")

    private fun get(url: String): JSONObject? = try {
        val c = URL(url).openConnection() as HttpURLConnection
        c.connectTimeout = 10000
        c.readTimeout = 15000
        c.setRequestProperty("Accept", "application/vnd.github+json")
        c.setRequestProperty("User-Agent", "Suppa/${U.version()}")
        val r = if (c.responseCode == 200) JSONObject(c.inputStream.bufferedReader().use { it.readText() }) else null
        c.disconnect()
        r
    } catch (e: Exception) {
        U.log("update check: $e")
        null
    }

    /** Start: after an update, What's new; once a day, look for a newer version. */
    fun onStart() {
        val last = U.readText(lastRunFile())?.trim() ?: ""
        if (last.isNotEmpty() && last != U.version()) U.later(1500) { whatsNew(U.version()) }
        U.writeAtomic(lastRunFile(), U.version())
        if (!Prefs.autoUpdate) return
        val at = U.readText(stampFile())?.trim()?.toLongOrNull() ?: 0
        if (System.currentTimeMillis() - at > 86_400_000) U.later(8000) { check(manual = false) }
    }

    fun check(manual: Boolean) {
        U.bg {
            U.writeAtomic(stampFile(), System.currentTimeMillis().toString())
            val rel = get("$API/latest")
            U.ui {
                if (rel == null) {
                    if (manual) Popup.message("Check for updates", "Could not reach GitHub. Try again later.")
                    return@ui
                }
                val tag = rel.optString("tag_name").removePrefix("v")
                val apk = rel.optJSONArray("assets")?.let { a ->
                    (0 until a.length()).map { a.getJSONObject(it) }.firstOrNull { it.optString("name").endsWith(".apk") }
                }
                if (!U.newer(tag, U.version())) {
                    if (manual) Popup.message("Suppa is up to date", "You have the newest version, ${U.version()}.")
                    return@ui
                }
                Bar.updateAvailable = true
                Bar.postUpdate()
                val notes = rel.optString("body").take(1200)
                if (apk == null) {
                    Popup.dialog("Suppa $tag is available", notes, listOf("Open the download page", "Later")) { i ->
                        if (i == 0) Tabs.newTab(PAGE)
                    }
                } else {
                    Popup.dialog("Suppa $tag is available", notes, listOf("Update now", "Later")) { i ->
                        if (i == 0) install(apk.optString("browser_download_url"), apk.optString("name"))
                    }
                }
            }
        }
    }

    private fun install(url: String, name: String) {
        val act = MainActivity.act
        if (!act.packageManager.canRequestPackageInstalls()) {
            Popup.dialog(
                "Allow Suppa to install its update",
                "Android asks once: turn on \"Allow from this source\" for Suppa, then come back and tap Update now again.",
                listOf("Open the setting", "Cancel"),
            ) { i ->
                if (i == 0) act.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${act.packageName}")))
            }
            return
        }
        Popup.message("Downloading the update", "Suppa downloads $name and then asks Android to install it.")
        Downloads.internal(url, name) { uri ->
            if (uri == null) {
                Popup.message("Update failed", "The download did not finish. Try again later.")
                return@internal
            }
            Tabs.saveSession()
            val i = Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            act.startActivity(i)
        }
    }

    /** The release notes of [version] from GitHub. */
    fun whatsNew(version: String) {
        U.bg {
            val rel = get("$API/tags/v$version") ?: get("$API/tags/$version")
            U.ui {
                if (rel == null) Popup.message("What's new in Suppa $version", "The release notes could not be loaded.\n$PAGE")
                else Popup.message("What's new in Suppa $version", rel.optString("body").take(2000).ifEmpty { "No notes for this version." })
            }
        }
    }
}
