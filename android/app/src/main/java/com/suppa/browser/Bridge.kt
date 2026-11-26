package com.suppa.browser

import android.net.Uri
import android.webkit.WebResourceResponse
import android.webkit.WebView
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import java.io.ByteArrayInputStream
import java.util.WeakHashMap

/**
 * Suppa's own pages (https://suppa.ui/, suppa.settings, suppa.history,
 * suppa.downloads, suppa.newtab) are answered from the scripts folder, like
 * the request interceptor on Windows. They talk to Suppa with
 * window.chrome.webview.postMessage(...) and get messages through its
 * "message" event, the same API as WebView2: SHIM provides it on Android on
 * top of a message channel that only these origins get (web sites never see it).
 */
object Bridge {
    val ORIGINS = setOf(
        "https://suppa.ui", "https://suppa.settings", "https://suppa.history",
        "https://suppa.downloads", "https://suppa.newtab",
    )

    private const val SHIM = """<script>(function(){
if(window.chrome&&window.chrome.webview)return;
var L=[],Q=[];window.chrome=window.chrome||{};
function host(){return window.suppaHost;}
function send(m){var h=host();var s=typeof m==='string'?m:JSON.stringify(m);if(h)h.postMessage(s);else Q.push(s);}
window.chrome.webview={postMessage:send,
 addEventListener:function(t,f){if(t==='message')L.push(f);},
 removeEventListener:function(t,f){var i=L.indexOf(f);if(i>=0)L.splice(i,1);}};
function hook(){var h=host();if(!h){setTimeout(hook,10);return;}
 h.onmessage=function(e){var d;try{d=JSON.parse(e.data);}catch(x){d=e.data;}
  for(var i=0;i<L.slice().length;i++){try{L[i]({data:d});}catch(x){console.error(x);}}};
 while(Q.length)h.postMessage(Q.shift());}
hook();})();</script>"""

    private val replies = WeakHashMap<WebView, JavaScriptReplyProxy>()

    /** Lets this view's Suppa pages talk to [onMessage] (view, origin host, text). */
    fun attach(view: WebView, onMessage: (WebView, String, String) -> Unit) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            U.log("WebView too old: no message listener (update Android System WebView)")
            return
        }
        WebViewCompat.addWebMessageListener(view, "suppaHost", ORIGINS) { v, msg, origin, isMain, reply ->
            if (!isMain) return@addWebMessageListener
            replies[v] = reply
            val text = msg.data ?: return@addWebMessageListener
            onMessage(v, origin.host ?: "", text)
        }
    }

    /** Sends a message (JSON text) to the Suppa page shown in [view]. */
    fun send(view: WebView?, json: String) {
        if (view == null) return
        U.ui {
            try {
                replies[view]?.postMessage(json)
            } catch (e: Exception) {
                U.log("bridge send failed: $e")
            }
        }
    }

    fun isSuppa(url: String) = U.hostOf(url).startsWith("suppa.")

    /** Interface scale for the chrome views (bar, menus, tab grid). */
    fun scale() = Prefs.uiScale / 100f

    /** Answers requests for Suppa's own pages; null for everything else. */
    fun serve(url: String, chrome: Boolean = false): WebResourceResponse? {
        val uri = Uri.parse(url)
        val host = uri.host ?: return null
        if (uri.scheme != "https" || !host.startsWith("suppa.")) return null
        val path = (uri.path ?: "/").trimStart('/')
        return when (host) {
            "suppa.ui" -> {
                if (path.startsWith("thumb/")) {
                    val id = path.removePrefix("thumb/").toIntOrNull() ?: 0
                    val bytes = Tabs.thumbFor(id) ?: return notFound()
                    return WebResourceResponse("image/jpeg", null, 200, "OK", mapOf("Cache-Control" to "no-store"), ByteArrayInputStream(bytes))
                }
                val name = path.ifEmpty { "ui.html" }
                if (name.endsWith(".html")) html(page(name), true) else file(name)
            }
            "suppa.settings" -> html(page("settings.html"), false)
            "suppa.history" -> html(page("history.html"), false)
            "suppa.downloads" -> html(page("downloads.html"), false)
            "suppa.newtab" -> html(NewTab.html(), false)
            else -> notFound()
        }
    }

    private fun page(name: String): String {
        var text = U.script(name) ?: "<!doctype html><p>Missing $name</p>"
        if (name == "ui.html") {
            // The phone parts of the bar, after the shared ui.css / ui.js.
            text = text.replace("<link rel=\"stylesheet\" href=\"ui.css\">", "<link rel=\"stylesheet\" href=\"ui.css\"><link rel=\"stylesheet\" href=\"mobile.css\">")
            text = text.replace("<script src=\"ui.js\"></script>", "<script src=\"ui.js\"></script><script src=\"mobile.js\"></script>")
        }
        if (name == "menu.html") {
            // A menu is never wider than the phone's screen (long titles end in "...").
            text = text.replace(
                "<div id=\"m\"></div>",
                "<div id=\"m\"></div><script>window.chrome.webview.addEventListener('message',function(e){" +
                    "if(e.data&&e.data.maxw)document.getElementById('m').style.maxWidth=e.data.maxw+'px';});</script>",
            )
        }
        return text
    }

    private fun html(text: String, chrome: Boolean): WebResourceResponse {
        // A phone viewport, and the message channel, before anything else on the page.
        val s = scale()
        val vp = if (chrome) {
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=$s, minimum-scale=$s, maximum-scale=$s, user-scalable=no\">"
        } else {
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
        }
        val i = text.indexOf("<head>")
        val out = if (i >= 0) text.substring(0, i + 6) + vp + SHIM + text.substring(i + 6) else vp + SHIM + text
        return WebResourceResponse("text/html", "utf-8", 200, "OK", mapOf("Cache-Control" to "no-store"), ByteArrayInputStream(out.toByteArray()))
    }

    private fun file(name: String): WebResourceResponse {
        val bytes = U.scriptBytes(name) ?: return notFound()
        val mime = when (name.substringAfterLast('.')) {
            "css" -> "text/css"
            "js" -> "application/javascript"
            "svg" -> "image/svg+xml"
            "png" -> "image/png"
            "json" -> "application/json"
            else -> "text/plain"
        }
        return WebResourceResponse(mime, "utf-8", 200, "OK", mapOf("Cache-Control" to "no-store"), ByteArrayInputStream(bytes))
    }

    private fun notFound() = WebResourceResponse("text/plain", "utf-8", 404, "Not Found", emptyMap(), ByteArrayInputStream(ByteArray(0)))
}

/** The start page (https://suppa.newtab/), filled in like newtab.rs does. */
object NewTab {
    const val URL = "https://suppa.newtab/"

    fun isNewTab(url: String) = url.isEmpty() || url.startsWith(URL) || url == "about:blank"

    fun html(): String {
        val t = U.script("newtab.html") ?: return ""
        return t.replace("{{SEARCH}}", Prefs.searchBase())
            .replace("{{THEME}}", themeCss())
            .replace("{{VERSION}}", U.version())
            .replace("{{SHORTCUTS}}", History.mostUsed(8).toString().replace("</", "<\\/"))
    }

    private fun themeCss(): String {
        val c = Prefs.palette().toMap()
        return ":root{--theme:${if (Prefs.dark) "dark" else "light"}}body{background:${c["bg"]};color:${c["fg"]}}" +
            "h1 span{color:${Prefs.accentColor()}}input{background:${c["field"]};border-color:${c["border"]}}" +
            "input:focus{border-color:${Prefs.accentColor()}}#tiles{grid-template-columns:repeat(4,1fr)}"
    }
}
