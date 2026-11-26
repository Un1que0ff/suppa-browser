package com.suppa.browser

import android.annotation.SuppressLint
import android.graphics.Color
import android.view.View
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import org.json.JSONArray
import org.json.JSONObject

/**
 * The tabs screen (the tabs button next to the address bar): every tab as a
 * tile with a picture of its page, like Chrome on a phone. scripts/tabs.html.
 * Tap = switch, × or swipe = close, long press = menu, hold and drag = sort,
 * drag onto another tile and hold = group.
 */
object Switcher {
    lateinit var view: WebView
    private var ready = false
    private var shown = false
    private var loadedScale = 0f
    /** Which tabs are shown: normal or incognito. */
    private var incognito = false

    @SuppressLint("SetJavaScriptEnabled")
    fun init(stage: FrameLayout) {
        view = WebView(stage.context).apply {
            settings.javaScriptEnabled = true
            setBackgroundColor(Color.parseColor(Prefs.color("bg")))
            visibility = View.GONE
            webViewClient = object : WebViewClient() {
                override fun shouldInterceptRequest(v: WebView, r: WebResourceRequest): WebResourceResponse? = Bridge.serve(r.url.toString(), true)
                override fun shouldOverrideUrlLoading(v: WebView, r: WebResourceRequest) = true
            }
        }
        Bridge.attach(view) { _, _, text -> onMessage(text) }
        stage.addView(view, FrameLayout.LayoutParams(-1, -1))
        load()
    }

    private fun load() {
        ready = false
        loadedScale = Bridge.scale()
        view.loadUrl("https://suppa.ui/tabs.html")
    }

    fun isOpen() = shown

    fun open() {
        if (shown) return
        Find.close()
        MainActivity.act.hideKeyboard()
        incognito = Tabs.activeTab()?.incognito ?: false
        val t = Tabs.activeTab()
        val show = {
            shown = true
            theme()
            refresh()
            view.visibility = View.VISIBLE
            view.bringToFront()
            view.requestFocus()
            Bridge.send(view, "{\"type\":\"shown\"}")
        }
        if (t != null) Tabs.captureThumb(t) { if (!shown) show() } else show()
        // The picture can take a moment; never wait long for it.
        U.later(250) { if (!shown) show() }
    }

    fun close() {
        if (!shown) return
        shown = false
        view.visibility = View.GONE
        Tabs.activeView()?.requestFocus()
    }

    fun theme() {
        if (loadedScale != Bridge.scale()) load()
        view.setBackgroundColor(Color.parseColor(Prefs.color("bg")))
        if (!ready) return
        Bridge.send(
            view,
            U.obj(
                "type" to "theme", "dark" to Prefs.dark, "accent" to Prefs.accentForBar(), "on_accent" to Prefs.onAccent,
                "colors" to Prefs.colorsJson(), "radius" to Prefs.radius, "squircle" to (Prefs.cornerShape == "squircle"),
                "icons" to Prefs.iconPack,
            ).toString(),
        )
    }

    fun refresh() {
        if (!ready || !shown) return
        val list = JSONArray()
        Tabs.tabs.forEachIndexed { i, t ->
            val fav = if (t.incognito || !t.favicon.startsWith("http")) "" else t.favicon
            list.put(
                U.obj(
                    "id" to t.id, "index" to i, "t" to Bar.tabTitle(t), "u" to (if (NewTab.isNewTab(t.url)) "" else t.url),
                    "f" to fav, "inc" to t.incognito, "g" to t.group, "s" to (if (t.loading) "loading" else if (t.view == null) "unloaded" else ""),
                    "th" to (if (t.thumb != null) "https://suppa.ui/thumb/${t.id}?v=${t.thumbVer}" else ""),
                ),
            )
        }
        Bridge.send(
            view,
            U.obj(
                "type" to "tabs", "tabs" to list, "groups" to Tabs.groupsJson(), "active" to Tabs.active, "inc" to incognito,
                "count" to Tabs.tabs.count { !it.incognito }, "count_inc" to Tabs.tabs.count { it.incognito },
            ).toString(),
        )
    }

    /** A new group asks for its name (inline, in the tab grid). */
    fun startRename(gid: Int) {
        if (gid == 0) return
        if (!shown) open()
        U.later(120) { Bridge.send(view, U.obj("type" to "group_rename", "id" to gid).toString()) }
    }

    /** Tab grid CSS px -> stage px. */
    private fun px(css: Double) = (css * Bridge.scale() * U.density()).toFloat()

    private fun onMessage(json: String) {
        val v = try { JSONObject(json) } catch (_: Exception) { return }
        val n = { k: String -> v.optInt(k) }
        when (v.optString("cmd")) {
            "ready" -> {
                ready = true
                theme()
                refresh()
            }
            "activate" -> {
                Tabs.activate(n("index"))
                close()
            }
            "close" -> Tabs.closeTab(n("index"))
            "new" -> {
                Tabs.newTab(null, true, v.optBoolean("inc"))
                close()
                U.later(150) { Bar.post(U.obj("type" to "focus_address")) }
            }
            "mode" -> {
                incognito = v.optBoolean("inc")
                refresh()
            }
            "menu" -> Menus.tab(n("index"), px(v.optDouble("x")), px(v.optDouble("y")), right = v.optDouble("x") > view.width / (2 * Bridge.scale() * U.density()))
            "group_menu" -> Menus.group(n("id"), px(v.optDouble("x")), px(v.optDouble("y")))
            "group_toggle" -> Tabs.toggleGroup(n("id"))
            "group_rename" -> Tabs.renameGroup(n("id"), v.optString("name"))
            "move" -> Tabs.move(n("from"), n("to"))
            "group" -> startRename(Tabs.groupWith(n("from"), n("target")))
            "more" -> Menus.switcherMenu(px(v.optDouble("x")), px(v.optDouble("y")), incognito)
            "done" -> if (Tabs.activeTab() != null) close()
        }
    }
}
