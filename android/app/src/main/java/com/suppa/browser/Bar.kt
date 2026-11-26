package com.suppa.browser

import android.webkit.WebView
import org.json.JSONArray
import org.json.JSONObject

/**
 * The bar: scripts/ui.html (+ the phone parts mobile.css / mobile.js) in a
 * WebView that fills the screen behind the page, exactly like on Windows.
 * Its rows sit at the top and/or bottom; the page covers the rest (#hole).
 */
object Bar {
    lateinit var view: WebView
    var ready = false
    var accent = "#b07cff"
    var bg = "#161616"
    var updateAvailable = false

    /** Bar CSS px -> screen px inside the stage. */
    fun px(css: Double): Float = (css * Bridge.scale() * U.density()).toFloat()

    fun post(json: String) {
        if (ready) Bridge.send(view, json)
    }

    fun post(o: JSONObject) = post(o.toString())

    fun onMessage(json: String) {
        val v = try { JSONObject(json) } catch (_: Exception) { return }
        val s = { k: String -> v.optString(k) }
        val n = { k: String -> v.optDouble(k, 0.0) }
        when (s("cmd")) {
            "ready" -> {
                ready = true
                U.log("startup: bar ready at ${System.currentTimeMillis() - U.startedAt} ms")
                pushTheme()
                sync()
                Saved.sync()
                post(U.obj("type" to "exts", "items" to JSONArray()))
                Downloads.syncBar()
                postUpdate()
            }
            "navigate" -> {
                Tabs.navigateActive(s("text"))
                MainActivity.act.hideKeyboard()
            }
            "activate" -> Tabs.activate(n("index").toInt())
            "close" -> Tabs.closeTab(n("index").toInt())
            "run" -> Commands.run(s("name"))
            "star" -> Saved.toggleActive()
            "menu" -> {
                val x = px(n("x"))
                val y = px(n("y"))
                when (s("which")) {
                    "adblock" -> Menus.adblock(x, y)
                    "downloads" -> Downloads.menu(x, y)
                    else -> Menus.main(x, y)
                }
            }
            "tab_menu" -> Menus.tab(n("index").toInt(), px(n("x")), px(n("y")))
            "tabs_menu" -> Menus.tabsButton(px(n("x")), px(n("y")))
            "tabs" -> Switcher.open()
            "saved_open", "saved_folder", "saved_menu", "saved_move", "saved_rename" -> Saved.onBarMessage(s("cmd"), v, px(n("x")), px(n("y")))
            "set_theme" -> {
                bg = s("bg").ifEmpty { bg }
                MainActivity.act.applyTheme(v.optBoolean("dark", true), bg)
            }
            "accent" -> accent = s("color")
            "focus_page" -> Tabs.activeView()?.requestFocus()
            "hole" -> MainActivity.act.setHole(px(n("top")), px(n("bottom")))
            "find" -> Find.onMessage(v)
            "rows" -> {}
        }
    }

    /** Theme, accent, sizes and layout for the bar (it answers with set_theme). */
    fun pushTheme() {
        val pos = U.obj("tabs" to "top", "nav" to Prefs.navPos, "saved" to Prefs.savedPos)
        post(
            U.obj(
                "type" to "init", "dark" to Prefs.dark, "accent" to Prefs.accentForBar(), "saved" to Prefs.showSaved,
                // No tab row on a phone: the tabs are in their own screen (the tabs button).
                "rows" to U.jarr(listOf(0, Prefs.rowNav, Prefs.rowSaved)), "locked" to true, "radius" to Prefs.radius,
                "colors" to Prefs.colorsJson(), "on_accent" to Prefs.onAccent,
                "order" to U.jarr(Prefs.rowOrder), "pos" to pos, "native_buttons" to true,
                "tab_corners" to Prefs.tabCorners, "squircle" to (Prefs.cornerShape == "squircle"),
                "theme" to Prefs.themeName(), "icons" to Prefs.iconPack, "scale" to Bridge.scale(),
            ),
        )
        Popup.theme()
        Switcher.theme()
    }

    fun postUpdate() {
        post(U.obj("type" to "update", "available" to updateAvailable, "building" to false))
    }

    fun progress(p: Int) {
        post(U.obj("type" to "progress", "p" to p))
    }

    fun tabTitle(t: Tab) = when {
        t.title.isNotEmpty() -> t.title
        NewTab.isNewTab(t.url) -> "New Tab"
        else -> U.siteOf(t.url).ifEmpty { t.url }
    }

    /** Tabs + address bar state. */
    fun sync() {
        if (!ready) return
        val t = Tabs.activeTab()
        val list = JSONArray()
        for (x in Tabs.tabs) {
            val st = if (x.loading) "loading" else if (x.view == null && !NewTab.isNewTab(x.url)) "unloaded" else ""
            val fav = if (x.incognito || !x.favicon.startsWith("http")) "" else x.favicon
            list.put(U.obj("id" to x.id, "t" to tabTitle(x), "s" to st, "i" to x.incognito, "f" to fav, "g" to x.group))
        }
        val url = t?.let { if (NewTab.isNewTab(it.url)) "" else it.url } ?: ""
        val host = U.hostOf(url)
        val ads = if (!Prefs.adblock) "off" else if (Prefs.isAllowed(host)) "site" else "on"
        val v = t?.view
        val inc = t?.incognito ?: false
        post(
            U.obj(
                "type" to "sync", "tabs" to list, "active" to Tabs.active, "url" to url, "blocked" to (t?.blocked ?: 0),
                "ads" to ads, "back" to (v?.canGoBack() ?: false), "fwd" to (v?.canGoForward() ?: false), "max" to true,
                "inc" to inc, "groups" to Tabs.groupsJson(), "star" to (url.isNotEmpty() && Saved.contains(url)),
                "count" to Tabs.tabs.count { it.incognito == inc }, "progress" to (if (t?.loading == true) t.progress else 100),
            ),
        )
    }
}
