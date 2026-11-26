package com.suppa.browser

import org.json.JSONObject
import java.io.File

/** settings.ini: the same file and keys as Suppa for Windows (settings.rs). */
object Prefs {
    val SEARCH_ENGINES = listOf(
        "Google" to "https://www.google.com/search",
        "DuckDuckGo" to "https://duckduckgo.com/",
        "Bing" to "https://www.bing.com/search",
        "Brave Search" to "https://search.brave.com/search",
    )
    const val DEFAULT_HOME = "https://www.google.com/"
    val COLOR_NAMES = listOf("bg", "panel", "field", "btn", "btn-hover", "tab", "tab-fg", "fg", "muted", "border", "incognito")

    // (min, default, max). The phone rows are a bit taller than on a PC (fingers).
    val ROW_NAV = Triple(40, 52, 80)
    val ROW_SAVED = Triple(22, 34, 56)
    val RADIUS = Triple(0, 10, 20)
    val UI_SCALE = Triple(75, 100, 150)
    val TINT = Triple(0, 0, 60)

    var search = 0
    var adblock = true
    var discardMinutes = 30
    var restoreSession = true
    var dark = true
    var theme = "dark"
    var home = DEFAULT_HOME
    var accent = "cycle"
    var autoCookies = true
    var autoUpdate = true
    var savePasswords = true
    var showSaved = false
    var dlButton = true
    var askDefault = true
    var rowNav = ROW_NAV.second
    var rowSaved = ROW_SAVED.second
    var radius = RADIUS.second
    var uiScale = UI_SCALE.second
    var onAccent = "auto"
    /** Address bar at the "top" or the "bottom" of the screen. */
    var navPos = "top"
    var savedPos = "top"
    var tint = "#ff0000"
    var tintAmount = 0
    var tabCorners = "auto"
    var cornerShape = "round"
    var iconPack = "line"
    var rowOrder = mutableListOf("tabs", "nav", "saved")
    val colorsDark = mutableListOf<Pair<String, String>>()
    val colorsLight = mutableListOf<Pair<String, String>>()
    val allow = mutableListOf<String>()
    var textZoom = 100

    private fun file() = File(U.dataDir(), "settings.ini")

    fun clamp(v: Int, r: Triple<Int, Int, Int>) = v.coerceIn(r.first, r.third)
    private fun validHex(v: String) = v.length == 7 && v.startsWith("#") && v.drop(1).all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }
    fun validPos(v: String) = if (v.trim() == "bottom") "bottom" else "top"
    fun validAccent(v: String): String {
        val t = v.trim()
        if (t == "windows") return t
        return if (validHex(t)) t.lowercase() else "cycle"
    }

    fun themeName(): String = when {
        dark && theme == "darker" -> "darker"
        !dark && (theme == "light" || theme == "lighter") -> theme
        dark -> "dark"
        else -> "light"
    }

    fun useDark(d: Boolean) {
        val extra = themeName() == "darker" || themeName() == "lighter"
        theme = when {
            d && extra -> "darker"
            d -> "dark"
            extra -> "lighter"
            else -> "light"
        }
        dark = d
    }

    fun useTheme(t: String) {
        theme = when (t.trim()) { "darker", "light", "lighter" -> t.trim(); else -> "dark" }
        dark = theme == "dark" || theme == "darker"
    }

    fun colors() = if (dark) colorsDark else colorsLight

    fun setColor(name: String, hex: String?) {
        if (name !in COLOR_NAMES) return
        val list = colors()
        list.removeAll { it.first == name }
        if (hex != null && validHex(hex)) list.add(name to hex.lowercase())
    }

    fun searchBase() = SEARCH_ENGINES[search.coerceIn(0, SEARCH_ENGINES.size - 1)].second

    fun isAllowed(host: String): Boolean {
        val h = host.removePrefix("www.")
        return h.isNotEmpty() && allow.any { h == it || h.endsWith(".$it") }
    }

    fun toggleAllow(host: String) {
        val h = host.removePrefix("www.")
        if (h.isEmpty()) return
        if (!allow.remove(h)) allow.add(h)
    }

    /** The phone's own accent colour (Material You), for the "system accent" swatch. */
    fun systemAccent(): String = try {
        val c = U.ctx.resources.getColor(android.R.color.system_accent1_500, null)
        String.format("#%06x", c and 0xffffff)
    } catch (_: Exception) {
        "#3584e4"
    }

    fun accentForBar() = if (accent == "windows") systemAccent() else accent

    /** The accent colour itself (the first rainbow colour for "cycle"). */
    fun accentColor(): String = when (accent) {
        "cycle" -> if (dark) "#b07cff" else "#7048e8"
        "windows" -> systemAccent()
        else -> accent
    }

    fun onAccentColor(bg: String = accentColor()): String = when (onAccent) {
        "black" -> "#1c1c1c"
        "white" -> "#ffffff"
        else -> {
            val n = bg.drop(1).toLongOrNull(16) ?: 0xb07cff
            val l = 0.299 * (n shr 16 and 255) + 0.587 * (n shr 8 and 255) + 0.114 * (n and 255)
            if (l > 160) "#1c1c1c" else "#ffffff"
        }
    }

    private val PALETTES = mapOf(
        //          bg        panel     field     btn       btn-hover tab       tab-fg    fg        muted     border    incognito
        "dark" to longArrayOf(0x161616, 0x1f1f1f, 0x272727, 0x2e2e2e, 0x3b3b3b, 0x2e2e2e, 0x8e8e8e, 0xececec, 0x8e8e8e, 0x383838, 0x5b4a86),
        "darker" to longArrayOf(0x000000, 0x0c0c0c, 0x161616, 0x1c1c1c, 0x2a2a2a, 0x1c1c1c, 0x8a8a8a, 0xf2f2f2, 0x8a8a8a, 0x262626, 0x463a6e),
        "light" to longArrayOf(0xf2f2f2, 0xffffff, 0xffffff, 0xe5e5e5, 0xd8d8d8, 0xe5e5e5, 0x6b6b6b, 0x1c1c1c, 0x6b6b6b, 0xd0d0d0, 0x6f5aa8),
        "lighter" to longArrayOf(0xf8f8f8, 0xffffff, 0xf6f6f6, 0xf0f0f0, 0xe6e6e6, 0xececec, 0x707070, 0x111111, 0x707070, 0xe2e2e2, 0x7c66c2),
    )

    /** Every interface colour: the theme, mixed with the tint, custom colours on top (ui.rs palette). */
    fun palette(): List<Pair<String, String>> {
        val base = PALETTES[themeName()] ?: PALETTES["dark"]!!
        val t = tint.drop(1).toLongOrNull(16) ?: 0xff0000
        val amount = tintAmount.coerceIn(0, 100) / 100.0
        fun mix(c: Long, a: Double): String {
            fun ch(shift: Int): Long {
                val x = (c shr shift and 255).toDouble()
                val y = (t shr shift and 255).toDouble()
                return Math.round(x + (y - x) * a)
            }
            return String.format("#%02x%02x%02x", ch(16), ch(8), ch(0))
        }
        val custom = colors()
        fun get(n: String) = custom.firstOrNull { it.first == n }?.second
        return COLOR_NAMES.mapIndexed { i, name ->
            val a = if (name == "fg" || name == "muted" || name == "tab-fg") 0.0 else amount
            val own = get(name) ?: when (name) { "tab" -> get("btn"); "tab-fg" -> get("muted"); else -> null }
            name to (own ?: mix(base[i], a))
        }
    }

    fun color(name: String) = palette().first { it.first == name }.second

    fun colorsJson() = JSONObject().apply { palette().forEach { put(it.first, it.second) } }
    fun customColorsJson() = JSONObject().apply { colors().forEach { put(it.first, it.second) } }

    fun load() {
        val text = U.readText(file()) ?: return
        for (line in text.lines()) {
            val i = line.indexOf('=')
            if (i < 0) continue
            val k = line.substring(0, i).trim()
            val v = line.substring(i + 1).trim()
            val n = v.toIntOrNull()
            when (k) {
                "search" -> search = (n ?: 0).coerceIn(0, SEARCH_ENGINES.size - 1)
                "adblock" -> adblock = v != "0"
                "discard_minutes" -> discardMinutes = n ?: 30
                "restore_session" -> restoreSession = v != "0"
                "theme" -> useTheme(v)
                "tint" -> tint = if (validHex(v)) v.lowercase() else "#ff0000"
                "tint_amount" -> tintAmount = clamp(n ?: 0, TINT)
                "tab_corners" -> tabCorners = if (v.length == 4 && v.all { it == '0' || it == '1' }) v else "auto"
                "corner_shape" -> cornerShape = if (v == "squircle") v else "round"
                "icon_pack" -> iconPack = if (v in listOf("bold", "sharp", "filled", "duotone")) v else "line"
                "home" -> home = v.ifEmpty { DEFAULT_HOME }
                "accent" -> accent = validAccent(v)
                "cookies" -> autoCookies = v != "0"
                "auto_update" -> autoUpdate = v != "0"
                "passwords" -> savePasswords = v != "0"
                "saved_bar" -> showSaved = v != "0"
                "dl_button" -> dlButton = v != "0"
                "ask_default" -> askDefault = v != "0"
                "row_nav" -> rowNav = clamp(n ?: ROW_NAV.second, ROW_NAV)
                "row_saved" -> rowSaved = clamp(n ?: ROW_SAVED.second, ROW_SAVED)
                "radius" -> radius = clamp(n ?: RADIUS.second, RADIUS)
                "ui_scale" -> uiScale = clamp(n ?: 100, UI_SCALE)
                "on_accent" -> onAccent = if (v == "black" || v == "white") v else "auto"
                "nav_pos" -> navPos = validPos(v)
                "saved_pos" -> savedPos = validPos(v)
                "row_order" -> {
                    val l = v.split(',').map { it.trim() }
                    if (l.size == 3 && listOf("tabs", "nav", "saved").all { r -> l.count { it == r } == 1 }) rowOrder = l.toMutableList()
                }
                "color_dark", "color_light" -> {
                    val j = v.indexOf(':')
                    if (j > 0) {
                        val name = v.substring(0, j)
                        val hex = v.substring(j + 1)
                        if (name in COLOR_NAMES && validHex(hex)) {
                            val list = if (k == "color_dark") colorsDark else colorsLight
                            list.removeAll { it.first == name }
                            list.add(name to hex.lowercase())
                        }
                    }
                }
                "allow" -> {
                    allow.clear()
                    allow.addAll(v.split(',').map { it.trim() }.filter { it.isNotEmpty() })
                }
                "text_zoom" -> textZoom = (n ?: 100).coerceIn(50, 300)
            }
        }
    }

    fun save() {
        val sb = StringBuilder("# Suppa Browser settings (Android)\n")
        fun kv(k: String, v: Any) = sb.append(k).append('=').append(v).append('\n')
        fun b(x: Boolean) = if (x) 1 else 0
        kv("search", search)
        kv("adblock", b(adblock))
        kv("discard_minutes", discardMinutes)
        kv("restore_session", b(restoreSession))
        kv("theme", themeName())
        kv("tint", tint)
        kv("tint_amount", tintAmount)
        kv("tab_corners", tabCorners)
        kv("corner_shape", cornerShape)
        kv("icon_pack", iconPack)
        kv("home", home)
        kv("accent", accent)
        kv("cookies", b(autoCookies))
        kv("auto_update", b(autoUpdate))
        kv("passwords", b(savePasswords))
        kv("saved_bar", b(showSaved))
        kv("dl_button", b(dlButton))
        kv("ask_default", b(askDefault))
        kv("row_nav", rowNav)
        kv("row_saved", rowSaved)
        kv("radius", radius)
        kv("ui_scale", uiScale)
        kv("on_accent", onAccent)
        kv("nav_pos", navPos)
        kv("saved_pos", savedPos)
        kv("row_order", rowOrder.joinToString(","))
        colorsDark.forEach { kv("color_dark", "${it.first}:${it.second}") }
        colorsLight.forEach { kv("color_light", "${it.first}:${it.second}") }
        kv("allow", allow.joinToString(","))
        kv("text_zoom", textZoom)
        U.writeAtomic(file(), sb.toString())
    }
}
