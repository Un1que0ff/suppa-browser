package com.suppa.browser

import android.annotation.SuppressLint
import android.app.Activity
import android.app.role.RoleManager
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.inputmethod.InputMethodManager
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout

/**
 * Suppa's one window. Layers, bottom to top:
 *   the bar (ui.html, fills the screen) -> the page (covers the bar's #hole)
 *   -> the tabs screen -> menus and dialogs -> a full-screen video.
 */
class MainActivity : Activity() {
    companion object {
        lateinit var act: MainActivity
    }

    lateinit var root: FrameLayout
    lateinit var stage: FrameLayout
    private lateinit var pages: FrameLayout
    private var fullscreenView: View? = null
    private var fullscreen = false
    private var holeTop = 0f
    private var holeBottom = 0f

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(saved: Bundle?) {
        super.onCreate(saved)
        act = this
        U.ctx = applicationContext
        Prefs.load()
        WebView.setWebContentsDebuggingEnabled(true)
        CookieManager.getInstance().setAcceptCookie(true)

        root = FrameLayout(this)
        root.setBackgroundColor(Color.parseColor(Prefs.color("bg")))
        stage = FrameLayout(this)
        root.addView(stage, FrameLayout.LayoutParams(-1, -1))
        setContentView(root)
        edgeToEdge()

        // The bar
        val bar = WebView(this)
        bar.settings.javaScriptEnabled = true
        bar.setBackgroundColor(Color.parseColor(Prefs.color("bg")))
        bar.isVerticalScrollBarEnabled = false
        bar.isHorizontalScrollBarEnabled = false
        bar.overScrollMode = View.OVER_SCROLL_NEVER
        bar.webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(v: WebView, r: WebResourceRequest): WebResourceResponse? = Bridge.serve(r.url.toString(), true)
            override fun shouldOverrideUrlLoading(v: WebView, r: WebResourceRequest) = true
        }
        Bar.view = bar
        Bridge.attach(bar) { _, _, text -> Bar.onMessage(text) }
        stage.addView(bar, FrameLayout.LayoutParams(-1, -1))

        // The page
        pages = FrameLayout(this)
        pages.setBackgroundColor(Color.parseColor(Prefs.color("bg")))
        stage.addView(pages, FrameLayout.LayoutParams(-1, -1).apply { topMargin = U.dp(52f) })
        Tabs.container = pages

        Switcher.init(stage)
        Popup.init(stage)

        bar.loadUrl("https://suppa.ui/ui.html")

        if (!handleIntent(intent) && !Tabs.restore()) Tabs.newTab()
        else if (Tabs.tabs.isEmpty()) Tabs.newTab()

        AdBlock.init()
        Scripts.watch()
        U.later(60_000) { Tabs.sleepTick() }
        Updater.onStart()
        U.later(2500) { askDefault() }
    }

    // ---- layout ------------------------------------------------------------------------

    private fun edgeToEdge() {
        window.setDecorFitsSystemWindows(false)
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT
        root.setOnApplyWindowInsetsListener { _, ins ->
            if (fullscreen) {
                stage.setPadding(0, 0, 0, 0)
            } else {
                val bars = ins.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                val ime = ins.getInsets(WindowInsets.Type.ime())
                stage.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
            }
            WindowInsets.CONSUMED
        }
    }

    /** Where the page goes: below the rows at the top, above the rows at the bottom (bar px). */
    fun setHole(top: Float, bottom: Float) {
        holeTop = top
        holeBottom = bottom
        if (fullscreen) return
        val lp = pages.layoutParams as FrameLayout.LayoutParams
        val t = Math.round(top)
        val b = Math.round(bottom)
        if (lp.topMargin != t || lp.bottomMargin != b) {
            lp.topMargin = t
            lp.bottomMargin = b
            pages.layoutParams = lp
        }
    }

    /** The bar's theme: the window around it, and dark or light system icons. */
    fun applyTheme(dark: Boolean, bg: String) {
        val c = try { Color.parseColor(bg) } catch (_: Exception) { Color.BLACK }
        root.setBackgroundColor(c)
        Bar.view.setBackgroundColor(c)
        window.insetsController?.setSystemBarsAppearance(
            if (dark) 0 else WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS,
            WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS,
        )
        applyPageBackground()
    }

    fun applyPageBackground() {
        val c = Color.parseColor(Prefs.color("bg"))
        pages.setBackgroundColor(c)
        Tabs.tabs.forEach { it.view?.setBackgroundColor(c) }
    }

    fun hideKeyboard() {
        val imm = getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager
        imm.hideSoftInputFromWindow(root.windowToken, 0)
        currentFocus?.clearFocus()
    }

    // ---- full screen (≡ -> Full screen, and videos) ----------------------------------------

    private fun systemBars(show: Boolean) {
        val c = window.insetsController ?: return
        if (show) c.show(WindowInsets.Type.systemBars())
        else {
            c.hide(WindowInsets.Type.systemBars())
            c.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    fun toggleFullscreen() {
        fullscreen = !fullscreen
        Bar.view.visibility = if (fullscreen) View.INVISIBLE else View.VISIBLE
        val lp = pages.layoutParams as FrameLayout.LayoutParams
        if (fullscreen) {
            lp.topMargin = 0
            lp.bottomMargin = 0
            pages.layoutParams = lp
        } else {
            setHole(holeTop, holeBottom)
        }
        systemBars(!fullscreen)
        root.requestApplyInsets()
        if (fullscreen) Popup.message("Full screen", "Swipe from the edge of the screen and tap Back to leave full screen.")
    }

    fun showFullscreenView(v: View) {
        fullscreenView = v
        v.setBackgroundColor(Color.BLACK)
        root.addView(v, FrameLayout.LayoutParams(-1, -1))
        systemBars(false)
    }

    fun hideFullscreenView() {
        fullscreenView?.let { root.removeView(it) }
        fullscreenView = null
        if (!fullscreen) systemBars(true)
    }

    // ---- back ----------------------------------------------------------------------------

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        val t = Tabs.activeTab()
        when {
            fullscreenView != null -> Tabs.activeView()?.webChromeClient?.onHideCustomView()
            Popup.isOpen() -> Popup.close()
            Find.isOpen() -> Find.close()
            Switcher.isOpen() -> Switcher.close()
            fullscreen -> toggleFullscreen()
            t?.view?.canGoBack() == true -> t.view?.goBack()
            // Opened from another tab: closing it goes back there (like Chrome).
            t != null && t.openerId != 0 && Tabs.byId(t.openerId) != null -> Tabs.closeTab(Tabs.active)
            else -> moveTaskToBack(true)
        }
    }

    // ---- links from other apps -------------------------------------------------------------

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(i: Intent?): Boolean {
        i ?: return false
        val url: String? = when (i.action) {
            Intent.ACTION_VIEW -> i.dataString
            Intent.ACTION_WEB_SEARCH -> i.getStringExtra("query")?.let { U.normalizeInput(it, Prefs.searchBase()) }
            Intent.ACTION_SEND -> i.getStringExtra(Intent.EXTRA_TEXT)?.let { text ->
                Regex("https?://\\S+").find(text)?.value ?: U.normalizeInput(text, Prefs.searchBase())
            }
            else -> null
        }
        if (url.isNullOrEmpty()) return false
        if (Tabs.tabs.isEmpty()) Tabs.restore()
        Tabs.newTab(url, true, false)
        Switcher.close()
        i.action = null
        return true
    }

    // ---- files, permissions, results -----------------------------------------------------

    private var nextReq = 100
    private val results = HashMap<Int, (Int, Intent?) -> Unit>()
    private val permResults = HashMap<Int, (Boolean) -> Unit>()

    private fun startForResult(i: Intent, cb: (Int, Intent?) -> Unit) {
        val code = nextReq++
        results[code] = cb
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(i, code)
        } catch (e: Exception) {
            results.remove(code)
            cb(RESULT_CANCELED, null)
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        results.remove(requestCode)?.invoke(resultCode, data)
    }

    /** A page's <input type=file>. */
    fun pickFiles(i: Intent, cb: (Array<Uri>?) -> Unit) {
        startForResult(i) { code, data ->
            if (code != RESULT_OK || data == null) { cb(null); return@startForResult }
            val clip = data.clipData
            val list = if (clip != null) (0 until clip.itemCount).map { clip.getItemAt(it).uri } else listOfNotNull(data.data)
            cb(list.toTypedArray())
        }
    }

    fun openDocument(mimes: Array<String>, cb: (Uri?) -> Unit) {
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*").putExtra(Intent.EXTRA_MIME_TYPES, mimes)
        startForResult(i) { code, data -> cb(if (code == RESULT_OK) data?.data else null) }
    }

    fun createDocument(name: String, mime: String, cb: (Uri?) -> Unit) {
        val i = Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(mime).putExtra(Intent.EXTRA_TITLE, name)
        startForResult(i) { code, data -> cb(if (code == RESULT_OK) data?.data else null) }
    }

    fun requestPerms(perms: Array<String>, cb: (Boolean) -> Unit) {
        val missing = perms.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isEmpty()) { cb(true); return }
        val code = nextReq++
        permResults[code] = cb
        requestPermissions(missing.toTypedArray(), code)
    }

    override fun onRequestPermissionsResult(code: Int, perms: Array<out String>, grants: IntArray) {
        super.onRequestPermissionsResult(code, perms, grants)
        permResults.remove(code)?.invoke(grants.any { it == PackageManager.PERMISSION_GRANTED })
    }

    // ---- default browser -------------------------------------------------------------------

    fun makeDefault() {
        val rm = getSystemService(ROLE_SERVICE) as RoleManager
        if (rm.isRoleAvailable(RoleManager.ROLE_BROWSER) && !rm.isRoleHeld(RoleManager.ROLE_BROWSER)) {
            startForResult(rm.createRequestRoleIntent(RoleManager.ROLE_BROWSER)) { _, _ -> PagesHost.broadcast() }
        } else {
            try { startActivity(Intent(Settings.ACTION_MANAGE_DEFAULT_APPS_SETTINGS)) } catch (_: Exception) { }
        }
    }

    private fun askDefault() {
        if (!Prefs.askDefault || PagesHost.isDefault() || Popup.isOpen()) return
        Popup.dialog("Make Suppa your default browser?", "Links from other apps then open in Suppa.", listOf("Make default", "Not now", "Don't ask again")) { i ->
            when (i) {
                0 -> makeDefault()
                2 -> { Prefs.askDefault = false; Prefs.save() }
            }
        }
    }

    // ---- life --------------------------------------------------------------------------

    override fun onPause() {
        super.onPause()
        Tabs.saveSession()
        CookieManager.getInstance().flush()
        Tabs.activeView()?.onPause()
    }

    override fun onResume() {
        super.onResume()
        Tabs.activeView()?.onResume()
    }

    /** Update / restore: Suppa starts again with the same tabs. */
    fun restart(saveSession: Boolean = true) {
        if (saveSession) Tabs.saveSession()
        val i = packageManager.getLaunchIntentForPackage(packageName) ?: return
        i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
        finishAffinity()
        startActivity(i)
        U.later(200) { Runtime.getRuntime().exit(0) }
    }

    override fun onDestroy() {
        if (isFinishing) Tabs.saveSession()
        super.onDestroy()
    }
}
