package com.suppa.browser

import android.webkit.WebView
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject
import java.io.File
import java.util.WeakHashMap

/**
 * Scripts that run on web pages at document start (like the WebView2 version):
 * page.js (passwords, downloads of blob: links, site-specific hiding of ads),
 * cookies.js, youtube.js, the generic ad-hiding style sheet and the user's
 * own scripts (.js files in scripts/userscripts). Pages talk back through "suppaPage", which only ever
 * gets answers meant for the page's own origin.
 */
object Content {
    private var cached: String? = null
    private val listening = WeakHashMap<WebView, Boolean>()

    fun invalidate() {
        cached = null
        Tabs.applySettings()
    }

    private fun script(): String {
        cached?.let { return it }
        val sb = StringBuilder()
        sb.append(U.script("page.js") ?: "")
        if (Prefs.autoCookies) sb.append("\n;").append(U.script("cookies.js") ?: "")
        if (Prefs.adblock) {
            sb.append("\n;").append(U.script("youtube.js") ?: "")
            val css = AdBlock.genericCss()
            if (css.isNotEmpty()) {
                val allow = U.jarr(Prefs.allow)
                sb.append("\n;(function(){var h=location.hostname.replace(/^www\\./,'');var A=$allow;")
                sb.append("if(A.some(function(a){return h===a||h.slice(-a.length-1)==='.'+a;}))return;")
                sb.append("var s=document.createElement('style');s.textContent=${U.jsStr(css)};")
                sb.append("(document.head||document.documentElement).appendChild(s);})();")
            }
        }
        sb.append(userScripts())
        return sb.toString().also { cached = it }
    }

    /** The .js files in scripts/userscripts (files starting with "_" are ignored), with their // @match lines. */
    private fun userScripts(): String {
        val dir = File(U.scriptsDir(), "userscripts")
        val files = dir.listFiles { f -> f.isFile && f.name.endsWith(".js") && !f.name.startsWith("_") } ?: return ""
        val sb = StringBuilder()
        for (f in files.sortedBy { it.name }) {
            val code = try { f.readText() } catch (_: Exception) { continue }
            val matches = Regex("""//\s*@match\s+(\S+)""").findAll(code).map { m ->
                "^" + m.groupValues[1].split('*').joinToString(".*") { seg ->
                    seg.replace(Regex("[.?+^$()\\[\\]{}|/\\\\]")) { "\\" + it.value }
                } + "$"
            }.toList()
            sb.append("\n;(function(){var M=${U.jarr(matches)};")
            sb.append("if(M.length&&!M.some(function(r){return new RegExp(r).test(location.href);}))return;try{\n")
            sb.append(code)
            sb.append("\n}catch(e){console.error('Suppa user script ${f.name}:',e);}})();")
        }
        return sb.toString()
    }

    fun attach(t: Tab, v: WebView) {
        if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
            t.scripts.forEach { try { it.remove() } catch (_: Exception) { } }
            t.scripts.clear()
            try {
                t.scripts.add(WebViewCompat.addDocumentStartJavaScript(v, script(), setOf("*")))
            } catch (e: Exception) {
                U.log("document start script: $e")
            }
        }
        if (listening[v] == true || !WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return
        listening[v] = true
        WebViewCompat.addWebMessageListener(v, "suppaPage", setOf("*")) { view, msg, origin, isMain, reply ->
            val tab = Tabs.tabs.firstOrNull { it.view === view } ?: return@addWebMessageListener
            val m = try { JSONObject(msg.data ?: return@addWebMessageListener) } catch (_: Exception) { return@addWebMessageListener }
            val originStr = origin.toString().trimEnd('/')
            val host = origin.host ?: ""
            when (m.optString("t")) {
                "hello" -> {
                    val pageHost = U.hostOf(tab.url)
                    if (Prefs.adblock && !Prefs.isAllowed(pageHost) && host.isNotEmpty()) {
                        val css = AdBlock.cosmeticCss(host)
                        if (css.isNotEmpty()) reply.postMessage(U.obj("t" to "css", "css" to css).toString())
                    }
                    if (Prefs.savePasswords && !tab.incognito && originStr.startsWith("http")) {
                        val items = Passwords.forOrigin(originStr)
                        if (items.length() > 0) reply.postMessage(U.obj("t" to "pw_fill", "items" to items).toString())
                    }
                    if (isMain) Downloads.pageReady(tab, reply)
                }
                "pw_submit" -> if (Prefs.savePasswords && !tab.incognito && originStr.startsWith("http")) {
                    Passwords.offerSave(originStr, view.url ?: originStr, m.optString("user"), m.optString("pass"))
                }
                "blob_data" -> Downloads.blobData(m.optString("token"), m.optString("data"), m.optString("name"))
            }
        }
    }
}
