package com.suppa.browser

import android.app.role.RoleManager
import android.content.Context
import android.webkit.CookieManager
import android.webkit.WebStorage
import android.webkit.WebView
import androidx.webkit.WebViewCompat
import org.json.JSONObject

/** Messages from Suppa's own pages: Settings, History, Downloads, start page (settings_page.rs + pages.rs). */
object PagesHost {
    fun engineVersion(): String = try {
        WebViewCompat.getCurrentWebViewPackage(U.ctx)?.versionName ?: ""
    } catch (_: Exception) {
        ""
    }

    fun isDefault(): Boolean = try {
        val rm = U.ctx.getSystemService(Context.ROLE_SERVICE) as RoleManager
        rm.isRoleHeld(RoleManager.ROLE_BROWSER)
    } catch (_: Exception) {
        false
    }

    fun state(): String = U.obj(
        "type" to "settings", "platform" to "android",
        "home" to Prefs.home, "search" to Prefs.search, "engines" to U.jarr(Prefs.SEARCH_ENGINES.map { it.first }),
        "dark" to Prefs.dark, "accent" to Prefs.accent, "radius" to Prefs.radius, "ui_scale" to Prefs.uiScale,
        "on_accent" to Prefs.onAccent, "bar_pos" to Prefs.navPos, "tabs_pos" to "top", "nav_pos" to Prefs.navPos,
        "saved_pos" to Prefs.savedPos, "row_order" to U.jarr(Prefs.rowOrder), "theme" to Prefs.themeName(),
        "tint" to Prefs.tint, "tint_amount" to Prefs.tintAmount, "tab_corners" to Prefs.tabCorners,
        "corner_shape" to Prefs.cornerShape, "icon_pack" to Prefs.iconPack, "colors" to Prefs.customColorsJson(),
        "palette" to Prefs.colorsJson(), "windows_accent" to Prefs.systemAccent(), "adblock" to Prefs.adblock,
        "cookies" to Prefs.autoCookies, "auto_update" to Prefs.autoUpdate, "passwords" to Prefs.savePasswords,
        "autofill" to false, "saved_bar" to Prefs.showSaved, "is_default" to isDefault(),
        "restore" to Prefs.restoreSession, "sleep" to 5, "discard" to Prefs.discardMinutes, "allow" to U.jarr(Prefs.allow),
        "extensions" to U.jarr(emptyList()), "version" to U.version(), "engine" to engineVersion(),
        "scripts" to U.scriptsDir().path, "data" to U.dataDir().path,
    ).toString()

    /** Every open Settings / History / Downloads page gets the new values (theme from the menu...). */
    fun broadcast() {
        for (t in Tabs.tabs) {
            val v = t.view ?: continue
            if (Bridge.isSuppa(t.url) && !NewTab.isNewTab(t.url)) Bridge.send(v, state())
        }
    }

    fun onMessage(t: Tab, view: WebView, host: String, json: String) {
        val v = try { JSONObject(json) } catch (_: Exception) { return }
        val cmd = v.optString("cmd")
        val reply = { o: Any -> Bridge.send(view, o.toString()) }
        when {
            cmd == "get" -> reply(state())
            host == "suppa.settings" && cmd == "set" -> {
                set(v.optString("key"), v.opt("value"))
                reply(state())
            }
            host == "suppa.settings" && cmd == "action" -> U.later(10) { Commands.run(v.optString("name")) }
            host == "suppa.settings" && cmd == "pw" -> Passwords.onPage(view, v)
            host == "suppa.history" && cmd == "list" -> reply(History.listJson(v.optString("q"), v.optInt("offset"), 200))
            host == "suppa.history" && cmd == "delete" -> {
                History.delete(v.optJSONArray("items"))
                reply("{\"type\":\"hist_changed\"}")
            }
            host == "suppa.history" && cmd == "clear" -> {
                val since = v.optLong("since")
                Popup.confirm(
                    if (since == 0L) "Delete all history?" else "Delete the last hour of history?", "This cannot be undone.", "Delete",
                ) {
                    History.clear(since)
                    reply("{\"type\":\"hist_changed\"}")
                }
            }
            host == "suppa.downloads" && cmd == "list" -> reply(Downloads.listJson())
            host == "suppa.downloads" && cmd == "dl" -> Downloads.op(v.optString("op"), v.optLong("id"))
            host == "suppa.newtab" && cmd == "hide_site" -> History.hideSite(v.optString("host"))
        }
    }

    /** One setting changed (Settings page or a menu). */
    fun set(key: String, value: Any?) {
        val b = value == true
        val n = when (value) {
            is Number -> value.toInt()
            is String -> value.toIntOrNull() ?: 0
            else -> 0
        }
        val text = (value as? String)?.trim() ?: ""
        var theme = false
        var content = false
        when (key) {
            "home" -> Prefs.home = if (text.isEmpty()) Prefs.DEFAULT_HOME else U.normalizeInput(text, Prefs.searchBase()).ifEmpty { Prefs.DEFAULT_HOME }
            "search" -> Prefs.search = n.coerceIn(0, Prefs.SEARCH_ENGINES.size - 1)
            "dark" -> { Prefs.useDark(b); theme = true }
            "theme" -> { Prefs.useTheme(text); theme = true }
            "tint" -> { Prefs.tint = if (Regex("#[0-9a-fA-F]{6}").matches(text)) text.lowercase() else "#ff0000"; theme = true }
            "tint_amount" -> { Prefs.tintAmount = Prefs.clamp(n, Prefs.TINT); theme = true }
            "tab_corners" -> { Prefs.tabCorners = if (text.length == 4 && text.all { it == '0' || it == '1' }) text else "auto"; theme = true }
            "corner_shape" -> { Prefs.cornerShape = if (text == "squircle") text else "round"; theme = true }
            "icon_pack" -> { Prefs.iconPack = if (text in listOf("bold", "sharp", "filled", "duotone")) text else "line"; theme = true }
            "row_pos" -> {
                val o = value as? JSONObject
                val pos = Prefs.validPos(o?.optString("pos") ?: "")
                when (o?.optString("row")) {
                    "nav" -> Prefs.navPos = pos
                    "saved" -> Prefs.savedPos = pos
                }
                theme = true
            }
            "bar_pos" -> { Prefs.navPos = Prefs.validPos(text); Prefs.savedPos = Prefs.navPos; theme = true }
            "accent" -> { Prefs.accent = Prefs.validAccent(text); theme = true }
            "radius" -> { Prefs.radius = Prefs.clamp(n, Prefs.RADIUS); theme = true }
            "row_order" -> {
                val l = text.split(',').map { it.trim() }
                Prefs.rowOrder = if (l.size == 3 && listOf("tabs", "nav", "saved").all { r -> l.count { it == r } == 1 }) l.toMutableList() else mutableListOf("tabs", "nav", "saved")
                theme = true
            }
            "on_accent" -> { Prefs.onAccent = if (text == "black" || text == "white") text else "auto"; theme = true }
            "ui_scale" -> { Prefs.uiScale = Prefs.clamp(n, Prefs.UI_SCALE); theme = true }
            "color" -> {
                val o = value as? JSONObject
                Prefs.setColor(o?.optString("name") ?: "", o?.optString("hex")?.takeIf { it.startsWith("#") })
                theme = true
            }
            "colors_reset" -> { Prefs.colors().clear(); theme = true }
            "adblock" -> { Prefs.adblock = b; content = true }
            "cookies" -> { Prefs.autoCookies = b; content = true }
            "passwords" -> Prefs.savePasswords = b
            "restore" -> Prefs.restoreSession = b
            "auto_update" -> Prefs.autoUpdate = b
            "saved_bar" -> { Prefs.showSaved = b; theme = true }
            "discard" -> Prefs.discardMinutes = n.coerceIn(0, 1440)
            "allow_remove" -> { Prefs.allow.remove(text); content = true }
            else -> return
        }
        Prefs.save()
        if (theme) {
            Bar.pushTheme()
            Saved.sync()
            MainActivity.act.applyPageBackground()
        }
        if (content) Content.invalidate()
        Bar.sync()
        broadcast()
    }

    fun clearData() {
        Popup.confirm(
            "Clear browsing data?",
            "History, cookies, cache, site data and saved passwords are deleted. Open tabs and saved tabs stay.",
            "Clear all",
        ) {
            CookieManager.getInstance().removeAllCookies(null)
            CookieManager.getInstance().flush()
            WebStorage.getInstance().deleteAllData()
            Tabs.tabs.firstNotNullOfOrNull { it.view }?.clearCache(true)
            History.clear(0)
            Passwords.deleteAll()
            Popup.message("Browsing data cleared", "")
        }
    }
}
