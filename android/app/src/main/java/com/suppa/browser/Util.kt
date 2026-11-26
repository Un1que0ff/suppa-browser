package com.suppa.browser

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.URLEncoder
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.Executors

/** Small helpers shared by everything (like util.rs on Windows). */
object U {
    lateinit var ctx: Context
    val main = Handler(Looper.getMainLooper())
    val io = Executors.newFixedThreadPool(3)
    val startedAt = System.currentTimeMillis()

    /** Settings, tabs, saved tabs, history... (private app storage). */
    fun dataDir(): File = ctx.filesDir

    /** The user's own scripts folder (files here override the built-in ones,
     *  like the scripts folder next to Suppa.exe):
     *  Android/data/com.suppa.browser/files/scripts */
    fun scriptsDir(): File = File(ctx.getExternalFilesDir(null) ?: ctx.filesDir, "scripts")

    fun ui(f: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) f() else main.post(f)
    }

    fun later(ms: Long = 0, f: () -> Unit) {
        main.postDelayed(f, ms)
    }

    fun bg(f: () -> Unit) {
        io.execute { try { f() } catch (e: Throwable) { log("background error: $e") } }
    }

    private val logFile by lazy { File(dataDir(), "suppa.log") }
    private val stamp = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.ROOT)

    fun log(msg: String) {
        android.util.Log.i("Suppa", msg)
        try {
            if (logFile.length() > 512 * 1024) logFile.writeText("")
            logFile.appendText("${stamp.format(Date())}  $msg\n")
        } catch (_: Exception) {
        }
    }

    fun readLog(): String = try { logFile.readText().takeLast(20000) } catch (_: Exception) { "" }

    /** A file of the scripts folder: the user's copy if there is one, else the built-in one. */
    fun script(name: String): String? {
        val own = File(scriptsDir(), name)
        if (own.isFile) return try { own.readText() } catch (_: Exception) { null }
        return try {
            ctx.assets.open(name).bufferedReader().use { it.readText() }
        } catch (_: Exception) {
            null
        }
    }

    fun scriptBytes(name: String): ByteArray? {
        val own = File(scriptsDir(), name)
        if (own.isFile) return try { own.readBytes() } catch (_: Exception) { null }
        return try { ctx.assets.open(name).use { it.readBytes() } } catch (_: Exception) { null }
    }

    fun readText(f: File): String? = try { if (f.isFile) f.readText() else null } catch (_: Exception) { null }

    /** Written next to it first, then renamed: a crash never leaves half a file. */
    fun writeAtomic(f: File, text: String) {
        try {
            f.parentFile?.mkdirs()
            val tmp = File(f.path + ".tmp")
            tmp.writeText(text)
            if (!tmp.renameTo(f)) {
                f.writeText(text)
                tmp.delete()
            }
        } catch (e: Exception) {
            log("could not write ${f.name}: $e")
        }
    }

    fun hostOf(url: String): String = try {
        Uri.parse(url).host?.lowercase() ?: ""
    } catch (_: Exception) {
        ""
    }

    fun siteOf(url: String): String = hostOf(url).removePrefix("www.")

    fun jsStr(s: String): String = JSONObject.quote(s)

    fun enc(s: String): String = URLEncoder.encode(s, "UTF-8")

    /** What was typed in the address bar -> an address (or a search). */
    fun normalizeInput(input: String, searchBase: String): String {
        val t = input.trim()
        if (t.isEmpty()) return ""
        val lower = t.lowercase()
        val schemes = listOf("http://", "https://", "file://", "about:", "chrome://", "view-source:", "data:", "javascript:", "intent:")
        if (schemes.any { lower.startsWith(it) }) return t
        if (lower.startsWith("localhost") || lower.startsWith("127.0.0.1")) return "http://$t"
        val first = t.split('/', '?', '#').first()
        val looksLikeHost = !t.contains(' ') && first.contains('.') && !first.startsWith('.') && !first.endsWith('.') &&
            first.all { it.isLetterOrDigit() || it == '.' || it == '-' || it == ':' }
        if (looksLikeHost) return "https://$t"
        return "$searchBase?q=${enc(t)}"
    }

    fun jarr(list: Iterable<Any?>): JSONArray = JSONArray().apply { list.forEach { put(it) } }

    fun obj(vararg kv: Pair<String, Any?>): JSONObject = JSONObject().apply {
        for ((k, v) in kv) put(k, v ?: JSONObject.NULL)
    }

    fun dp(v: Float): Int = (v * ctx.resources.displayMetrics.density + 0.5f).toInt()

    fun density(): Float = ctx.resources.displayMetrics.density

    fun version(): String = BuildConfig.VERSION_NAME

    /** "1.2.3" > "1.2.0" */
    fun newer(a: String, b: String): Boolean {
        val pa = a.trim().removePrefix("v").split('.', '-').map { it.toIntOrNull() ?: 0 }
        val pb = b.trim().removePrefix("v").split('.', '-').map { it.toIntOrNull() ?: 0 }
        for (i in 0 until maxOf(pa.size, pb.size)) {
            val x = pa.getOrElse(i) { 0 }
            val y = pb.getOrElse(i) { 0 }
            if (x != y) return x > y
        }
        return false
    }
}
