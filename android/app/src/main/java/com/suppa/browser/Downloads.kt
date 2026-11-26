package com.suppa.browser

import android.app.DownloadManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import android.util.Base64
import android.webkit.CookieManager
import android.webkit.URLUtil
import androidx.webkit.JavaScriptReplyProxy
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.WeakHashMap

/** Downloads (downloads.rs): Android's download manager, the download button, the Downloads page. */
object Downloads {
    class Dl(
        var id: Long, var name: String, var url: String, var uri: String, var t: Long, var state: String,
        var total: Long = -1, var got: Long = 0, var mime: String = "", var dm: Long = -1,
    )

    private val list = mutableListOf<Dl>()
    private var loaded = false
    private var nextLocal = -1L
    private val dm by lazy { U.ctx.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager }
    private val proxies = WeakHashMap<Tab, JavaScriptReplyProxy>()
    private val pendingBlobs = mutableMapOf<String, String>()
    private val onDone = mutableMapOf<Long, (Uri?) -> Unit>()
    private fun file() = File(U.dataDir(), "downloads.json")

    private fun items(): MutableList<Dl> {
        if (!loaded) {
            loaded = true
            val arr = try { JSONArray(U.readText(file()) ?: "[]") } catch (_: Exception) { JSONArray() }
            for (i in 0 until arr.length()) {
                val o = arr.optJSONObject(i) ?: continue
                list.add(Dl(o.optLong("id"), o.optString("name"), o.optString("url"), o.optString("uri"), o.optLong("t"), o.optString("state"),
                    o.optLong("total", -1), o.optLong("got"), o.optString("mime"), o.optLong("dm", -1)))
            }
            nextLocal = minOf(-1L, (list.minOfOrNull { it.id } ?: 0L) - 1)
            if (list.any { it.state == "active" }) poll()
        }
        return list
    }

    private fun save() {
        val arr = JSONArray()
        items().take(300).forEach {
            arr.put(U.obj("id" to it.id, "name" to it.name, "url" to it.url, "uri" to it.uri, "t" to it.t, "state" to it.state,
                "total" to it.total, "got" to it.got, "mime" to it.mime, "dm" to it.dm))
        }
        U.writeAtomic(file(), arr.toString())
    }

    /** The page's script is ready: blob: downloads of this tab go through it. */
    fun pageReady(t: Tab, reply: JavaScriptReplyProxy) {
        proxies[t] = reply
    }

    fun start(t: Tab?, url: String, ua: String, cd: String?, mime: String?) {
        val name = URLUtil.guessFileName(url, cd, mime)
        when {
            url.startsWith("blob:") -> {
                val p = t?.let { proxies[it] }
                if (p == null) {
                    Popup.message("Download failed", "This page's file could not be saved.")
                    return
                }
                val token = java.util.UUID.randomUUID().toString()
                pendingBlobs[token] = name
                p.postMessage(U.obj("t" to "blob", "url" to url, "token" to token, "name" to name).toString())
            }
            url.startsWith("data:") -> saveData(url, name)
            url.startsWith("http") -> enqueue(url, name, ua, mime ?: "", t?.url ?: "")
            else -> Popup.message("Download failed", "Suppa can't download this kind of address.")
        }
    }

    private fun enqueue(url: String, name: String, ua: String, mime: String, referer: String) {
        try {
            val r = DownloadManager.Request(Uri.parse(url))
                .setTitle(name)
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                .setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, name)
            CookieManager.getInstance().getCookie(url)?.let { r.addRequestHeader("Cookie", it) }
            if (ua.isNotEmpty()) r.addRequestHeader("User-Agent", ua)
            if (referer.startsWith("http")) r.addRequestHeader("Referer", referer)
            if (mime.isNotEmpty()) r.setMimeType(mime)
            val id = dm.enqueue(r)
            items().add(0, Dl(id, name, url, "", System.currentTimeMillis(), "active", mime = mime, dm = id))
            started()
        } catch (e: Exception) {
            U.log("download failed: $e")
            Popup.message("Download failed", e.message ?: "")
        }
    }

    /** Suppa's own downloads (an update): to Suppa's folder, then [done]. */
    fun internal(url: String, name: String, done: (Uri?) -> Unit) {
        val r = DownloadManager.Request(Uri.parse(url))
            .setTitle(name)
            .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE)
            .setDestinationInExternalFilesDir(U.ctx, Environment.DIRECTORY_DOWNLOADS, name)
        val id = dm.enqueue(r)
        onDone[id] = done
        poll()
    }

    private fun started() {
        save()
        syncBar()
        refreshPages()
        Bar.post("{\"type\":\"dl_open\"}")
        poll()
    }

    private fun saveData(dataUrl: String, name0: String) {
        val comma = dataUrl.indexOf(',')
        if (comma < 0) return
        val meta = dataUrl.substring(5, comma)
        val mime = meta.substringBefore(';').ifEmpty { "application/octet-stream" }
        val payload = dataUrl.substring(comma + 1)
        val bytes = if (meta.endsWith(";base64")) Base64.decode(payload, Base64.DEFAULT) else Uri.decode(payload).toByteArray()
        var name = name0
        if (!name.contains('.')) android.webkit.MimeTypeMap.getSingleton().getExtensionFromMimeType(mime)?.let { name += ".$it" }
        saveBytes(name, mime, bytes, dataUrl.take(60))
    }

    fun blobData(token: String, data: String, name: String) {
        U.ui {
            val n = pendingBlobs.remove(token) ?: return@ui
            if (data.isEmpty()) Popup.message("Download failed", "This page's file could not be read.")
            else saveData(data, name.ifEmpty { n })
        }
    }

    private fun saveBytes(name: String, mime: String, bytes: ByteArray, url: String) {
        U.bg {
            val cv = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, name)
                put(MediaStore.Downloads.MIME_TYPE, mime)
                put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
            }
            val uri = U.ctx.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, cv)
            if (uri != null) U.ctx.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
            U.ui {
                items().add(0, Dl(nextLocal--, name, url, uri?.toString() ?: "", System.currentTimeMillis(), if (uri != null) "done" else "failed",
                    bytes.size.toLong(), bytes.size.toLong(), mime))
                started()
            }
        }
    }

    // ---- progress -----------------------------------------------------------------------

    private var polling = false

    private fun poll() {
        if (polling) return
        polling = true
        U.later(700) { tick() }
    }

    private fun tick() {
        polling = false
        val active = items().filter { it.state == "active" && it.dm >= 0 }
        val ids = active.map { it.dm } + onDone.keys
        if (ids.isEmpty()) return
        var changed = false
        try {
            dm.query(DownloadManager.Query().setFilterById(*ids.toLongArray()))?.use { c ->
                val seen = mutableSetOf<Long>()
                while (c.moveToNext()) {
                    val id = c.getLong(c.getColumnIndexOrThrow(DownloadManager.COLUMN_ID))
                    seen.add(id)
                    val status = c.getInt(c.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
                    val got = c.getLong(c.getColumnIndexOrThrow(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR))
                    val total = c.getLong(c.getColumnIndexOrThrow(DownloadManager.COLUMN_TOTAL_SIZE_BYTES))
                    onDone[id]?.let { cb ->
                        if (status == DownloadManager.STATUS_SUCCESSFUL || status == DownloadManager.STATUS_FAILED) {
                            onDone.remove(id)
                            cb(if (status == DownloadManager.STATUS_SUCCESSFUL) dm.getUriForDownloadedFile(id) else null)
                        }
                    }
                    val d = active.firstOrNull { it.dm == id } ?: continue
                    val state = when (status) {
                        DownloadManager.STATUS_SUCCESSFUL -> "done"
                        DownloadManager.STATUS_FAILED -> "failed"
                        else -> "active"
                    }
                    if (state != d.state || got != d.got || total != d.total) changed = true
                    d.state = state
                    d.got = got
                    d.total = total
                    if (state == "done") d.uri = dm.getUriForDownloadedFile(id)?.toString() ?: ""
                }
                // Removed from Android's download list (e.g. cancelled in the notification).
                active.filter { it.dm !in seen }.forEach { it.state = "cancelled"; changed = true }
            }
        } catch (e: Exception) {
            U.log("download progress: $e")
        }
        if (changed) {
            save()
            refreshPages()
        }
        syncBar()
        if (items().any { it.state == "active" } || onDone.isNotEmpty()) poll()
    }

    /** The download button next to the address bar: shown or not, and how far the downloads are. */
    fun syncBar() {
        val act = items().filter { it.state == "active" }
        val total = act.sumOf { maxOf(0L, it.total) }
        val got = act.sumOf { it.got }
        val p = if (act.isNotEmpty() && act.all { it.total > 0 } && total > 0) got.toDouble() / total else -1.0
        Bar.post(U.obj("type" to "dl", "show" to Prefs.dlButton, "active" to act.size, "p" to p))
    }

    private fun size(b: Long): String = when {
        b < 0 -> "?"
        b < 1024 -> "$b B"
        b < 1024 * 1024 -> String.format("%.0f KB", b / 1024.0)
        b < 1024L * 1024 * 1024 -> String.format("%.1f MB", b / 1048576.0)
        else -> String.format("%.2f GB", b / 1073741824.0)
    }

    private fun status(d: Dl): String = when (d.state) {
        "active" -> if (d.total > 0) "Downloading ${d.got * 100 / d.total}% - ${size(d.got)} of ${size(d.total)}" else "Downloading - ${size(d.got)}"
        "done" -> "Done - ${size(d.total)}"
        "failed" -> "Failed"
        else -> "Cancelled"
    }

    fun listJson(): String {
        val arr = JSONArray()
        for (d in items()) {
            arr.put(U.obj("id" to d.id, "name" to d.name, "url" to d.url, "path" to d.name, "t" to d.t, "state" to d.state,
                "status" to status(d), "exists" to (d.state == "done" && d.uri.isNotEmpty()), "total" to d.total, "got" to d.got,
                "can_pause" to false))
        }
        return U.obj("type" to "dl_list", "items" to arr).toString()
    }

    private fun refreshPages() {
        val json = listJson()
        Tabs.tabs.filter { U.hostOf(it.url) == "suppa.downloads" }.forEach { Bridge.send(it.view, json) }
    }

    private fun openFile(d: Dl) {
        try {
            val i = Intent(Intent.ACTION_VIEW).setDataAndType(Uri.parse(d.uri), d.mime.ifEmpty { U.ctx.contentResolver.getType(Uri.parse(d.uri)) ?: "*/*" })
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            MainActivity.act.startActivity(i)
        } catch (e: Exception) {
            Popup.message("Can't open ${d.name}", "No app on this phone opens this kind of file.")
        }
    }

    private fun openFolder() {
        try {
            MainActivity.act.startActivity(Intent(DownloadManager.ACTION_VIEW_DOWNLOADS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (_: Exception) {
        }
    }

    /** Buttons on the Downloads page. */
    fun op(op: String, id: Long) {
        val d = items().firstOrNull { it.id == id }
        when (op) {
            "open" -> d?.let { openFile(it) }
            "folder", "open_folder" -> openFolder()
            "cancel" -> d?.let { if (it.dm >= 0) dm.remove(it.dm); it.state = "cancelled" }
            "retry" -> d?.let { start(Tabs.activeTab(), it.url, Tabs.activeView()?.settings?.userAgentString ?: "", null, it.mime) }
            "remove" -> d?.let { items().remove(it) }
            "clear" -> items().removeAll { it.state != "active" }
        }
        save()
        refreshPages()
        syncBar()
    }

    /** The download button: the latest downloads. */
    fun menu(x: Float, y: Float) {
        val recent = items().take(8)
        val out = mutableListOf<Item>()
        if (recent.isEmpty()) out.add(Item(0, "No downloads yet").enabled(false))
        recent.forEach { d ->
            out.add(Item(1000 + items().indexOf(d).toLong(), d.name).key(status(d).substringBefore(" - ").replace("Downloading ", "")).enabled(d.state == "done" || d.state == "active"))
        }
        out.add(Item.sep())
        out.add(Item(1, "All downloads"))
        Popup.show("downloads", out, x, y) { id ->
            val k = id?.toInt() ?: return@show
            if (k == 1) Commands.run("downloads") else items().getOrNull(k - 1000)?.let { if (it.state == "done") openFile(it) else Commands.run("downloads") }
        }
    }

    fun onTabsJson(): JSONObject = JSONObject()
}
