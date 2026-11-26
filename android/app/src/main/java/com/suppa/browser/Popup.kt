package com.suppa.browser

import android.annotation.SuppressLint
import android.app.AlertDialog
import android.graphics.Color
import android.text.InputType
import android.view.View
import android.view.ViewGroup
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import org.json.JSONArray
import org.json.JSONObject

/** One menu item (popup.rs Item). */
class Item(var id: Long = 0, var label: String = "") {
    var key = ""
    var icon = ""
    var checked = false
    var enabled = true
    var sep = false
    var hot = false
    var drag = false
    var heading = false
    var para = false
    var sub = listOf<Item>()
    var ctx = listOf<Item>()

    fun key(k: String) = apply { key = k }
    fun icon(i: String) = apply { icon = i }
    fun checked(c: Boolean) = apply { checked = c }
    fun enabled(e: Boolean) = apply { enabled = e }
    fun hot(h: Boolean) = apply { hot = h }
    fun draggable() = apply { drag = true }
    fun ctx(items: List<Item>) = apply { ctx = items }

    fun json(): JSONObject = U.obj(
        "id" to id, "l" to label, "k" to key, "i" to icon, "c" to checked, "e" to enabled, "s" to sep, "h" to hot,
        "d" to drag, "H" to heading, "P" to para,
        "sub" to JSONArray().apply { sub.forEach { put(it.json()) } },
        "ctx" to JSONArray().apply { ctx.forEach { put(it.json()) } },
    )

    companion object {
        fun sep() = Item().apply { sep = true }
        fun sub(label: String, items: List<Item>) = Item(-1, label).apply { sub = items }
        fun heading(t: String) = Item(0, t).apply { heading = true }
        fun text(t: String) = Item(0, t).apply { para = true }
    }
}

/**
 * Themed menus and dialogs: scripts/menu.html in a small WebView over
 * everything else, the same page as the popup window on Windows.
 */
object Popup {
    private lateinit var stage: FrameLayout
    private lateinit var scrim: View
    lateinit var view: WebView
    private var ready = false
    private var queued: String? = null
    private var onPick: ((Long?) -> Unit)? = null
    private var onReorder: ((Int, Int) -> Unit)? = null
    private var ax = 0f
    private var ay = 0f
    private var right = true
    private var center = false
    private var key = ""
    private var visible = false
    private var hiddenKey = ""
    private var hiddenAt = 0L
    private var loadedScale = 0f

    @SuppressLint("SetJavaScriptEnabled")
    fun init(stage: FrameLayout) {
        this.stage = stage
        scrim = View(stage.context).apply {
            setBackgroundColor(Color.TRANSPARENT)
            visibility = View.GONE
            setOnClickListener { finish(null) }
        }
        stage.addView(scrim, FrameLayout.LayoutParams(-1, -1))
        view = WebView(stage.context).apply {
            settings.javaScriptEnabled = true
            setBackgroundColor(Color.TRANSPARENT)
            visibility = View.INVISIBLE
            isVerticalScrollBarEnabled = false
            webViewClient = object : WebViewClient() {
                override fun shouldInterceptRequest(v: WebView, r: WebResourceRequest): WebResourceResponse? = Bridge.serve(r.url.toString(), true)
                override fun shouldOverrideUrlLoading(v: WebView, r: WebResourceRequest) = true
            }
            elevation = U.dp(12f).toFloat()
        }
        Bridge.attach(view) { _, _, text -> onMessage(text) }
        stage.addView(view, FrameLayout.LayoutParams(U.dp(240f), U.dp(100f)))
        load()
    }

    private fun load() {
        ready = false
        loadedScale = Bridge.scale()
        view.loadUrl("https://suppa.ui/menu.html")
    }

    /** Theme changed: the next menu uses it; a new interface scale reloads the page. */
    fun theme() {
        if (loadedScale != Bridge.scale()) load()
        view.setBackgroundColor(Color.TRANSPARENT)
    }

    fun isOpen() = visible || onPick != null

    private fun showMsg(items: List<Item>): String = U.obj(
        "type" to "show", "items" to JSONArray().apply { items.forEach { put(it.json()) } }, "dark" to Prefs.dark,
        "maxw" to ((stage.width - stage.paddingLeft - stage.paddingRight) / (Bridge.scale() * U.density()) - 16).toInt(),
        "accent" to Prefs.accentColor(), "on_accent" to Prefs.onAccent, "colors" to Prefs.colorsJson(),
        "radius" to Prefs.radius, "squircle" to (Prefs.cornerShape == "squircle"),
    ).toString()

    /**
     * Shows a menu hanging from (x, y) in stage pixels: its right edge there
     * ([right]) or its left edge; [center] = a dialog in the middle of the screen.
     */
    fun show(
        key: String, items: List<Item>, x: Float, y: Float, right: Boolean = true, center: Boolean = false,
        reorder: ((Int, Int) -> Unit)? = null, pick: (Long?) -> Unit,
    ) {
        // Tapping the button of the menu that was just closed by that tap: stays closed.
        if (!visible && key == hiddenKey && System.currentTimeMillis() - hiddenAt < 350) return
        if (onPick != null) finish(null, notify = true)
        this.key = key
        ax = x
        ay = y
        this.right = right
        this.center = center
        onPick = pick
        onReorder = reorder
        MainActivity.act.hideKeyboard()
        val msg = showMsg(items)
        if (ready) Bridge.send(view, msg) else queued = msg
    }

    /** Replaces the items of the open menu (after a reorder). */
    fun refresh(items: List<Item>) {
        Bridge.send(view, U.obj("type" to "items", "items" to JSONArray().apply { items.forEach { put(it.json()) } }).toString())
    }

    fun close() = finish(null)

    private fun finish(id: Long?, notify: Boolean = true) {
        if (visible) {
            hiddenKey = key
            hiddenAt = System.currentTimeMillis()
        }
        visible = false
        view.visibility = View.INVISIBLE
        scrim.visibility = View.GONE
        Bridge.send(view, "{\"type\":\"hide\"}")
        val cb = onPick
        onPick = null
        onReorder = null
        if (notify) cb?.invoke(id)
    }

    private fun place(wCss: Double, hCss: Double) {
        val f = Bridge.scale() * U.density()
        val m = U.dp(8f)
        val sw = stage.width - stage.paddingLeft - stage.paddingRight
        val sh = stage.height - stage.paddingTop - stage.paddingBottom
        val w = (wCss * f).toInt().coerceAtMost(sw - 2 * m)
        val h = (hCss * f).toInt().coerceAtMost(sh - 2 * m)
        var x: Float
        var y: Float
        if (center) {
            x = (sw - w) / 2f
            y = (sh - h) / 2f
        } else {
            x = if (right) ax - w else ax
            y = ay
            if (y + h > sh - m) y = (sh - m - h).toFloat()
        }
        x = x.coerceIn(m.toFloat(), (sw - m - w).toFloat().coerceAtLeast(m.toFloat()))
        y = y.coerceIn(m.toFloat(), (sh - m - h).toFloat().coerceAtLeast(m.toFloat()))
        val lp = view.layoutParams as FrameLayout.LayoutParams
        lp.width = w
        lp.height = h
        view.layoutParams = lp
        // (x, y) are inside the stage's padding (system bars), like the bar's coordinates.
        view.x = stage.paddingLeft + x
        view.y = stage.paddingTop + y
        // Cut to the menu's rounded box (the popup window shape on Windows).
        val r = ((if (Prefs.cornerShape == "squircle") Prefs.radius * 1.6 else Prefs.radius.toDouble()) + 3) * f
        view.outlineProvider = object : android.view.ViewOutlineProvider() {
            override fun getOutline(v: View, o: android.graphics.Outline) = o.setRoundRect(0, 0, v.width, v.height, r.toFloat())
        }
        view.clipToOutline = true
        scrim.setBackgroundColor(if (center) Color.argb(90, 0, 0, 0) else Color.TRANSPARENT)
        scrim.visibility = View.VISIBLE
        scrim.bringToFront()
        view.bringToFront()
        view.visibility = View.VISIBLE
        visible = true
        Bridge.send(view, "{\"type\":\"focus\"}")
    }

    private fun onMessage(json: String) {
        val v = try { JSONObject(json) } catch (_: Exception) { return }
        when (v.optString("cmd")) {
            "ready" -> {
                ready = true
                queued?.let { Bridge.send(view, it) }
                queued = null
            }
            "size" -> if (onPick != null) place(v.optDouble("w"), v.optDouble("h"))
            "pick" -> finish(v.optLong("id"))
            "close" -> finish(null)
            "reorder" -> onReorder?.invoke(v.optInt("from"), v.optInt("to"))
            "drag_out" -> {}
        }
    }

    // ---- dialogs ---------------------------------------------------------------------------

    /** A dialog: title, text and buttons (the first is the main one). [cb] gets the button's index, -1 = closed. */
    fun dialog(title: String, text: String, buttons: List<String>, center: Boolean = true, cb: (Int) -> Unit = {}) {
        val items = mutableListOf(Item.heading(title))
        if (text.isNotEmpty()) items.add(Item.text(text))
        buttons.forEachIndexed { i, b -> items.add(Item(i.toLong(), b).hot(i == 0)) }
        val st = stage
        show("dialog", items, st.width / 2f, st.height / 3f, center = center) { id -> cb(id?.toInt() ?: -1) }
    }

    fun message(title: String, text: String) = dialog(title, text, listOf("OK"))

    fun confirm(title: String, text: String, yes: String, cb: () -> Unit) {
        dialog(title, text, listOf(yes, "Cancel")) { if (it == 0) cb() }
    }

    private fun themedDialog(): AlertDialog.Builder = AlertDialog.Builder(
        MainActivity.act,
        if (Prefs.dark) android.R.style.Theme_DeviceDefault_Dialog_Alert else android.R.style.Theme_DeviceDefault_Light_Dialog_Alert,
    )

    private fun field(hint: String, value: String = "", password: Boolean = false) = EditText(MainActivity.act).apply {
        setText(value)
        this.hint = hint
        isSingleLine = true
        inputType = if (password) InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD else InputType.TYPE_CLASS_TEXT
        selectAll()
    }

    private fun box(vararg views: View) = LinearLayout(MainActivity.act).apply {
        orientation = LinearLayout.VERTICAL
        val p = U.dp(20f)
        setPadding(p, U.dp(8f), p, 0)
        views.forEach { addView(it, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)) }
    }

    /** Asks for a line of text (renaming, a page's prompt()). null = cancelled. */
    fun prompt(title: String, value: String, cb: (String?) -> Unit) {
        val f = field("", value)
        var done = false
        themedDialog().setTitle(title).setView(box(f))
            .setPositiveButton("OK") { _, _ -> done = true; cb(f.text.toString()) }
            .setNegativeButton("Cancel") { _, _ -> }
            .setOnDismissListener { if (!done) cb(null) }
            .show()
        f.requestFocus()
    }

    /** A site asks for a user name and password (HTTP sign-in). */
    fun login(host: String, cb: (String?, String?) -> Unit) {
        val u = field("User name")
        val p = field("Password", password = true)
        var done = false
        themedDialog().setTitle("Sign in to $host").setView(box(u, p))
            .setPositiveButton("Sign in") { _, _ -> done = true; cb(u.text.toString(), p.text.toString()) }
            .setNegativeButton("Cancel") { _, _ -> }
            .setOnDismissListener { if (!done) cb(null, null) }
            .show()
    }
}
