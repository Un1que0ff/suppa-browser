package com.suppa.browser

import android.annotation.SuppressLint
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Rect
import android.net.Uri
import android.os.Message
import android.view.MotionEvent
import android.view.PixelCopy
import android.view.View
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.HttpAuthHandler
import android.webkit.JsPromptResult
import android.webkit.JsResult
import android.webkit.PermissionRequest
import android.webkit.RenderProcessGoneDetail
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.webkit.ProfileStore
import androidx.webkit.ScriptHandler
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONArray
import java.io.ByteArrayOutputStream
import java.io.File

class Tab(val id: Int, var url: String, val incognito: Boolean) {
    var view: WebView? = null
    var title = ""
    var favicon = ""
    var loading = false
    var progress = 100
    var group = 0
    var blocked = 0
    var openerId = 0
    var lastActive = System.currentTimeMillis()
    var desktop = false
    var thumb: ByteArray? = null
    var thumbVer = 0
    val scripts = mutableListOf<ScriptHandler>()
}

class Group(val id: Int, var name: String, var color: String, var collapsed: Boolean)

/** Open tabs, the active one, tab groups, closed tabs, the session (tabs.rs + groups.rs). */
object Tabs {
    val tabs = mutableListOf<Tab>()
    var active = 0
    val groups = mutableListOf<Group>()
    private var nextId = 1
    private var nextGroup = 1
    private data class Closed(val url: String, val index: Int, val incognito: Boolean, val title: String)
    private val closed = ArrayDeque<Closed>()
    lateinit var container: FrameLayout

    /** At most this many pages stay loaded (phones have little memory). */
    private const val MAX_LIVE = 6

    val GROUP_COLORS = listOf(
        "Purple" to "#b07cff", "Green" to "#4cd37b", "Cyan" to "#3cd6e0", "Yellow" to "#ffd43b",
        "Red" to "#ff8a8a", "Blue" to "#5b9dff", "Orange" to "#ff9a3c", "Pink" to "#ff6fb5",
    )

    fun activeTab(): Tab? = tabs.getOrNull(active)
    fun activeView(): WebView? = activeTab()?.view
    fun indexOf(t: Tab) = tabs.indexOf(t)
    fun byId(id: Int) = tabs.firstOrNull { it.id == id }
    fun hasClosed() = closed.isNotEmpty()
    private fun tabOf(v: WebView) = tabs.firstOrNull { it.view === v }

    // ---- open / close / switch ------------------------------------------------------------

    fun newTab(url: String? = null, activate: Boolean = true, incognito: Boolean = false, at: Int = -1, opener: Tab? = null): Tab {
        val t = Tab(nextId++, url ?: Prefs.home, incognito)
        if (opener != null) t.openerId = opener.id
        val idx = if (at in 0..tabs.size) at else tabs.size
        tabs.add(idx, t)
        if (opener != null) inheritGroup(idx, opener)
        if (activate) activate(idx) else {
            if (idx <= active && tabs.size > 1) active++
            // A link opened in the background starts loading right away (like Chrome).
            if (url != null) {
                load(t, createView(t))
                trimLive()
            }
            changed()
        }
        return t
    }

    fun newIncognito(url: String? = null) = newTab(url ?: Prefs.home, true, incognito = true)

    fun activate(i: Int) {
        if (i !in tabs.indices) return
        val old = activeTab()
        if (old != null && old !== tabs[i]) {
            captureThumb(old)
            old.view?.let { it.onPause(); container.removeView(it) }
        }
        active = i
        val t = tabs[i]
        t.lastActive = System.currentTimeMillis()
        groups.firstOrNull { it.id == t.group }?.collapsed = false
        val v = t.view ?: createView(t).also { load(t, it) }
        if (v.parent !== container) {
            (v.parent as? ViewGroup)?.removeView(v)
            container.addView(v, FrameLayout.LayoutParams(-1, -1))
        }
        v.onResume()
        trimLive()
        changed()
    }

    fun closeTab(i: Int) {
        if (i !in tabs.indices) return
        val t = tabs[i]
        if (!t.incognito && !NewTab.isNewTab(t.url)) {
            closed.addLast(Closed(t.url, i, false, t.title))
            if (closed.size > 25) closed.removeFirst()
        }
        val wasActive = i == active
        // Going back to the tab that opened it (like Chrome), else the neighbour.
        val opener = if (wasActive && t.openerId != 0) byId(t.openerId) else null
        destroyView(t)
        tabs.removeAt(i)
        cleanupGroups()
        if (tabs.isEmpty()) {
            if (t.incognito) dropIncognitoProfile()
            newTab()
            return
        }
        if (t.incognito && tabs.none { it.incognito }) dropIncognitoProfile()
        if (wasActive) {
            val next = opener?.let { tabs.indexOf(it) } ?: -1
            active = -1
            activate(if (next >= 0) next else (i - 1).coerceIn(0, tabs.size - 1).let { if (i < tabs.size) i else it })
        } else {
            if (i < active) active--
            changed()
        }
    }

    fun closeAll(incognito: Boolean) {
        val keep = tabs.filter { it.incognito != incognito }
        val drop = tabs.filter { it.incognito == incognito }
        for (t in drop) {
            if (!t.incognito && !NewTab.isNewTab(t.url)) closed.addLast(Closed(t.url, 0, false, t.title))
            destroyView(t)
        }
        tabs.clear()
        tabs.addAll(keep)
        if (incognito) dropIncognitoProfile()
        cleanupGroups()
        active = -1
        if (tabs.isEmpty()) newTab() else activate(0)
    }

    fun closeOthers(i: Int) {
        val keep = tabs.getOrNull(i) ?: return
        for (t in tabs.toList()) if (t !== keep && t.incognito == keep.incognito) {
            if (!t.incognito) closed.addLast(Closed(t.url, 0, false, t.title))
            destroyView(t)
            tabs.remove(t)
        }
        cleanupGroups()
        active = -1
        activate(tabs.indexOf(keep))
    }

    fun reopenClosed() {
        val c = closed.removeLastOrNull() ?: return
        newTab(c.url, true, c.incognito, c.index.coerceAtMost(tabs.size))
    }

    fun duplicate(i: Int) {
        val t = tabs.getOrNull(i) ?: return
        val n = newTab(t.url, true, t.incognito, i + 1)
        n.group = t.group
        changed()
    }

    fun move(from: Int, to: Int) {
        if (from !in tabs.indices || to !in tabs.indices || from == to) return
        val activeTab = activeTab()
        val t = tabs.removeAt(from)
        tabs.add(to, t)
        afterMove(to)
        active = tabs.indexOf(activeTab)
        changed()
    }

    fun reload(i: Int) {
        val t = tabs.getOrNull(i) ?: return
        val v = t.view
        if (v == null) activate(i) else v.reload()
    }

    /** Address bar Enter. */
    fun navigateActive(text: String) {
        val url = U.normalizeInput(text, Prefs.searchBase())
        if (url.isEmpty()) return
        val t = activeTab() ?: return
        t.url = url
        val v = t.view ?: createView(t)
        if (v.parent == null) activate(active)
        v.loadUrl(url)
        v.requestFocus()
        changed()
    }

    fun openUrl(url: String, how: String) {
        when (how) {
            "new" -> newTab(url, true, activeTab()?.incognito ?: false, active + 1, activeTab())
            "bg" -> newTab(url, false, activeTab()?.incognito ?: false, active + 1, activeTab())
            "inc" -> newIncognito(url)
            else -> navigateActive(url)
        }
    }

    // ---- WebViews ----------------------------------------------------------------------

    private fun load(t: Tab, v: WebView) {
        if (t.url.isNotEmpty()) v.loadUrl(t.url) else v.loadUrl(Prefs.home)
    }

    private fun destroyView(t: Tab) {
        val v = t.view ?: return
        t.view = null
        t.scripts.clear()
        container.removeView(v)
        try {
            v.stopLoading()
            v.destroy()
        } catch (_: Exception) {
        }
    }

    /** Unloads a tab: frees its memory, it loads again when shown (sleeper.rs). */
    fun unload(t: Tab) {
        if (t === activeTab() || t.view == null) return
        t.view?.url?.let { if (it.startsWith("http") || it.startsWith("https")) t.url = it }
        destroyView(t)
        changed()
    }

    fun unloadBackground() {
        tabs.filter { it !== activeTab() }.forEach { unload(it) }
    }

    private fun trimLive() {
        val live = tabs.filter { it.view != null && it !== activeTab() }.sortedBy { it.lastActive }
        val extra = live.size - (MAX_LIVE - 1)
        if (extra > 0) live.take(extra).forEach { unload(it) }
    }

    /** Every minute: tabs not looked at for a while are unloaded (Settings -> Tabs and memory). */
    fun sleepTick() {
        val limit = Prefs.discardMinutes
        if (limit > 0) {
            val now = System.currentTimeMillis()
            tabs.filter { it !== activeTab() && it.view != null && now - it.lastActive > limit * 60_000L }.forEach { unload(it) }
        }
        U.later(60_000) { sleepTick() }
    }

    private var incognitoProfile = false

    private fun useIncognitoProfile(v: WebView): Boolean {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) return false
        return try {
            ProfileStore.getInstance().getOrCreateProfile("incognito")
            WebViewCompat.setProfile(v, "incognito")
            incognitoProfile = true
            true
        } catch (e: Exception) {
            U.log("incognito profile: $e")
            false
        }
    }

    /** The last incognito tab closed: its cookies, cache and site data go away. */
    private fun dropIncognitoProfile() {
        if (!incognitoProfile) return
        U.later(300) {
            if (tabs.any { it.incognito }) return@later
            try {
                ProfileStore.getInstance().deleteProfile("incognito")
                incognitoProfile = false
            } catch (e: Exception) {
                U.log("could not delete the incognito profile: $e")
            }
        }
    }

    fun userAgent(v: WebView, desktop: Boolean): String {
        // Without "; wv" sites treat Suppa like Chrome (some refuse "WebViews").
        val ua = WebSettings.getDefaultUserAgent(v.context).replace("; wv)", ")").replace(Regex("Version/\\d+\\.\\d+ "), "")
        if (!desktop) return ua
        val chrome = Regex("Chrome/[\\d.]+").find(ua)?.value ?: "Chrome/130.0.0.0"
        return "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) $chrome Safari/537.36"
    }

    @SuppressLint("SetJavaScriptEnabled", "ClickableViewAccessibility")
    fun createView(t: Tab): WebView {
        val act = MainActivity.act
        val v = WebView(act)
        if (t.incognito && !useIncognitoProfile(v)) U.log("incognito tab shares cookies (WebView without profiles)")
        t.view = v
        v.setBackgroundColor(android.graphics.Color.parseColor(Prefs.color("bg")))
        with(v.settings) {
            javaScriptEnabled = true
            domStorageEnabled = true
            databaseEnabled = true
            loadWithOverviewMode = true
            useWideViewPort = true
            builtInZoomControls = true
            displayZoomControls = false
            setSupportZoom(true)
            setSupportMultipleWindows(true)
            javaScriptCanOpenWindowsAutomatically = false
            mediaPlaybackRequiresUserGesture = true
            mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
            setGeolocationEnabled(true)
            allowFileAccess = false
            textZoom = Prefs.textZoom
            userAgentString = userAgent(v, t.desktop)
            if (t.incognito) cacheMode = WebSettings.LOAD_NO_CACHE
            saveFormData = false
        }
        CookieManager.getInstance().setAcceptThirdPartyCookies(v, true)
        Bridge.attach(v) { view, host, text -> tabOf(view)?.let { PagesHost.onMessage(it, view, host, text) } }
        Content.attach(t, v)
        v.webViewClient = PageClient()
        v.webChromeClient = PageChrome()
        v.setDownloadListener { url, ua, cd, mime, _ -> Downloads.start(t, url, ua, cd, mime) }
        // Long press on a link or image: Suppa's own menu.
        v.setOnTouchListener { _, e ->
            if (e.actionMasked == MotionEvent.ACTION_DOWN) {
                lastTouch[0] = e.x
                lastTouch[1] = e.y
            }
            false
        }
        v.setOnLongClickListener { Menus.pageContext(t, v, lastTouch[0], lastTouch[1]) }
        return v
    }

    private val lastTouch = FloatArray(2)

    /** Settings changed (text size, ad blocker...): applied to the loaded pages. */
    fun applySettings() {
        for (t in tabs) {
            val v = t.view ?: continue
            v.settings.textZoom = Prefs.textZoom
            Content.attach(t, v)
        }
    }

    fun setDesktop(t: Tab, on: Boolean) {
        t.desktop = on
        val v = t.view ?: return
        v.settings.userAgentString = userAgent(v, on)
        v.settings.loadWithOverviewMode = true
        v.reload()
    }

    private class PageClient : WebViewClient() {
        override fun shouldOverrideUrlLoading(view: WebView, req: WebResourceRequest): Boolean {
            val url = req.url.toString()
            val scheme = req.url.scheme?.lowercase() ?: ""
            if (scheme == "http" || scheme == "https" || scheme == "about" || scheme == "data" || scheme == "blob") return false
            // intent:, mailto:, tel:, market: ... -> the app that handles it
            try {
                val intent = if (scheme == "intent") Intent.parseUri(url, Intent.URI_INTENT_SCHEME) else Intent(Intent.ACTION_VIEW, req.url)
                intent.addCategory(Intent.CATEGORY_BROWSABLE)
                intent.component = null
                intent.selector = null
                MainActivity.act.startActivity(intent)
            } catch (e: Exception) {
                if (scheme == "intent") {
                    val fallback = try { Intent.parseUri(url, Intent.URI_INTENT_SCHEME).getStringExtra("browser_fallback_url") } catch (_: Exception) { null }
                    if (fallback != null) view.loadUrl(fallback)
                }
            }
            return true
        }

        override fun shouldInterceptRequest(view: WebView, req: WebResourceRequest): WebResourceResponse? {
            val url = req.url.toString()
            Bridge.serve(url)?.let { return it }
            if (req.isForMainFrame) return null
            val t = tabs.firstOrNull { it.view === view } ?: return null
            if (!Prefs.adblock) return null
            val page = pageUrl[view] ?: return null
            if (Prefs.isAllowed(U.hostOf(page))) return null
            if (AdBlock.shouldBlock(url, page, req.requestHeaders["Accept"] ?: "")) {
                t.blocked++
                requestSync()
                return WebResourceResponse("text/plain", "utf-8", 200, "OK", emptyMap(), java.io.ByteArrayInputStream(ByteArray(0)))
            }
            return null
        }

        override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
            val t = tabOf(view) ?: return
            pageUrl[view] = url
            t.url = url
            t.loading = true
            t.blocked = 0
            t.favicon = ""
            changed()
        }

        override fun onPageFinished(view: WebView, url: String) {
            val t = tabOf(view) ?: return
            t.loading = false
            t.progress = 100
            if (url.startsWith("http")) t.url = url
            pageUrl[view] = url
            // The site's icon address (like WebView2's FaviconUri).
            view.evaluateJavascript(
                "(function(){var l=document.querySelector('link[rel~=\"icon\"],link[rel=\"apple-touch-icon\"]');return l?l.href:(location.protocol.indexOf('http')===0?location.origin+'/favicon.ico':'');})()",
            ) { r ->
                val icon = r?.trim('"') ?: ""
                if (icon.startsWith("http")) {
                    t.favicon = icon
                    if (!t.incognito) {
                        History.setIcon(t.url, icon)
                        Saved.learnIcon(t.url, icon)
                    }
                    changed()
                }
            }
            if (t === activeTab()) U.later(400) { captureThumb(t) }
            changed()
        }

        override fun doUpdateVisitedHistory(view: WebView, url: String, isReload: Boolean) {
            val t = tabOf(view) ?: return
            t.url = url
            pageUrl[view] = url
            if (!t.incognito && url.startsWith("http") && !isReload) History.visit(url, view.title ?: "")
            changed()
        }

        override fun onReceivedHttpAuthRequest(view: WebView, handler: HttpAuthHandler, host: String, realm: String) {
            Popup.login(host) { user, pass -> if (user == null) handler.cancel() else handler.proceed(user, pass) }
        }

        override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
            // The page's engine process crashed or was killed: only this tab reloads.
            val t = tabOf(view)
            U.log("page process gone (crash=${detail.didCrash()}) ${t?.url}")
            if (t != null) {
                destroyView(t)
                if (t === activeTab()) U.later(100) { activate(tabs.indexOf(t)) }
                changed()
            } else {
                (view.parent as? ViewGroup)?.removeView(view)
                view.destroy()
            }
            return true
        }
    }

    /** The main-frame address of each view (read on the request thread). */
    private val pageUrl = java.util.concurrent.ConcurrentHashMap<WebView, String>()

    private class PageChrome : WebChromeClient() {
        override fun onReceivedTitle(view: WebView, title: String?) {
            val t = tabOf(view) ?: return
            t.title = title ?: ""
            if (!t.incognito && t.url.startsWith("http")) History.setTitle(t.url, t.title)
            changed()
        }

        override fun onProgressChanged(view: WebView, p: Int) {
            val t = tabOf(view) ?: return
            t.progress = p
            if (t === activeTab()) Bar.progress(p)
        }

        override fun onCreateWindow(view: WebView, isDialog: Boolean, isUserGesture: Boolean, resultMsg: Message): Boolean {
            val opener = tabOf(view) ?: return false
            if (!isUserGesture && !opener.loading) return false   // no pop-ups without a tap
            val t = Tab(nextId++, "", opener.incognito)
            t.openerId = opener.id
            val idx = tabs.indexOf(opener) + 1
            tabs.add(idx, t)
            inheritGroup(idx, opener)
            val v = createView(t)
            (resultMsg.obj as WebView.WebViewTransport).webView = v
            resultMsg.sendToTarget()
            activate(idx)
            return true
        }

        override fun onCloseWindow(window: WebView) {
            tabOf(window)?.let { closeTab(tabs.indexOf(it)) }
        }

        private var custom: View? = null
        private var customCb: CustomViewCallback? = null

        override fun onShowCustomView(v: View, cb: CustomViewCallback) {
            custom = v
            customCb = cb
            MainActivity.act.showFullscreenView(v)
        }

        override fun onHideCustomView() {
            custom = null
            customCb = null
            MainActivity.act.hideFullscreenView()
        }

        override fun onShowFileChooser(view: WebView, cb: ValueCallback<Array<Uri>>, params: FileChooserParams): Boolean {
            val intent = try { params.createIntent() } catch (_: Exception) { return false }
            if (params.mode == FileChooserParams.MODE_OPEN_MULTIPLE) intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
            MainActivity.act.pickFiles(intent) { uris -> cb.onReceiveValue(uris) }
            return true
        }

        override fun onPermissionRequest(req: PermissionRequest) {
            U.ui { Perms.webRequest(req) }
        }

        override fun onGeolocationPermissionsShowPrompt(origin: String, cb: GeolocationPermissions.Callback) {
            Perms.location(origin, cb)
        }

        override fun onJsAlert(view: WebView, url: String, message: String, result: JsResult): Boolean {
            Popup.dialog(U.siteOf(url).ifEmpty { "This page" } + " says", message, listOf("OK"), center = true) { result.confirm() }
            return true
        }

        override fun onJsConfirm(view: WebView, url: String, message: String, result: JsResult): Boolean {
            Popup.dialog(U.siteOf(url).ifEmpty { "This page" } + " asks", message, listOf("OK", "Cancel"), center = true) { i ->
                if (i == 0) result.confirm() else result.cancel()
            }
            return true
        }

        override fun onJsPrompt(view: WebView, url: String, message: String, def: String?, result: JsPromptResult): Boolean {
            Popup.prompt(message, def ?: "") { v -> if (v == null) result.cancel() else result.confirm(v) }
            return true
        }

        override fun getDefaultVideoPoster(): Bitmap = Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888)
    }

    // ---- thumbnails for the tab grid ------------------------------------------------------

    fun thumbFor(id: Int): ByteArray? = byId(id)?.thumb

    /** A small picture of the page as it is on screen (PixelCopy of the window). */
    fun captureThumb(t: Tab, done: (() -> Unit)? = null) {
        val v = t.view
        // Only the page itself: not while the tabs screen or a menu covers it.
        if (v == null || !v.isAttachedToWindow || v.width < 10 || v.height < 10 || Switcher.isOpen() || Popup.isOpen()) {
            done?.invoke()
            return
        }
        val loc = IntArray(2)
        v.getLocationInWindow(loc)
        val w = v.width
        val h = v.height
        val scale = 360f / w
        val bmp = try { Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888) } catch (_: Throwable) { done?.invoke(); return }
        try {
            PixelCopy.request(MainActivity.act.window, Rect(loc[0], loc[1], loc[0] + w, loc[1] + h), bmp, { res ->
                if (res == PixelCopy.SUCCESS) {
                    U.bg {
                        val small = Bitmap.createScaledBitmap(bmp, 360, (h * scale).toInt().coerceAtLeast(1), true)
                        val out = ByteArrayOutputStream()
                        small.compress(Bitmap.CompressFormat.JPEG, 72, out)
                        bmp.recycle()
                        small.recycle()
                        val bytes = out.toByteArray()
                        U.ui {
                            t.thumb = bytes
                            t.thumbVer++
                            Switcher.refresh()
                            done?.invoke()
                        }
                    }
                } else {
                    bmp.recycle()
                    done?.invoke()
                }
            }, U.main)
        } catch (e: Exception) {
            done?.invoke()
        }
    }

    // ---- groups (groups.rs) ---------------------------------------------------------------

    private fun newGroup(): Int {
        val id = nextGroup++
        val used = groups.map { it.color }
        val color = GROUP_COLORS.map { it.second }.firstOrNull { it !in used } ?: GROUP_COLORS[(id - 1) % GROUP_COLORS.size].second
        groups.add(Group(id, "", color, false))
        return id
    }

    fun cleanupGroups() {
        val used = tabs.map { it.group }.filter { it != 0 }.toSet()
        groups.removeAll { it.id !in used }
    }

    private fun keepActive(t: Tab?) {
        if (t != null) active = tabs.indexOf(t).coerceAtLeast(0)
    }

    /** Dropped tab [from] on tab [target] (and held): they form a group. Returns a new group's id. */
    fun groupWith(from: Int, target: Int): Int {
        if (from !in tabs.indices || target !in tabs.indices || from == target) return 0
        val act = activeTab()
        var created = 0
        val gid = if (tabs[target].group == 0) newGroup().also { tabs[target].group = it; created = it } else tabs[target].group
        val t = tabs.removeAt(from)
        t.group = gid
        val last = tabs.indexOfLast { it.group == gid }
        tabs.add(last + 1, t)
        keepActive(act)
        groups.firstOrNull { it.id == gid }?.collapsed = false
        cleanupGroups()
        changed()
        return created
    }

    private fun afterMove(idx: Int) {
        val left = if (idx > 0) tabs[idx - 1].group else 0
        val right = tabs.getOrNull(idx + 1)?.group ?: 0
        val own = tabs[idx].group
        tabs[idx].group = when {
            left != 0 && left == right -> left
            own != 0 && (own == left || own == right) -> own
            else -> 0
        }
        cleanupGroups()
    }

    private fun inheritGroup(idx: Int, opener: Tab) {
        if (opener.group != 0) tabs[idx].group = opener.group
    }

    fun addToNewGroup(idx: Int): Int {
        if (idx !in tabs.indices) return 0
        val act = activeTab()
        val id = newGroup()
        val old = tabs[idx].group
        val t = tabs.removeAt(idx)
        t.group = id
        val at = if (old != 0) tabs.indexOfLast { it.group == old }.let { if (it >= 0) it + 1 else idx } else idx
        tabs.add(at.coerceAtMost(tabs.size), t)
        keepActive(act)
        cleanupGroups()
        changed()
        return id
    }

    fun removeFromGroup(idx: Int) {
        val old = tabs.getOrNull(idx)?.group ?: return
        if (old == 0) return
        val act = activeTab()
        val t = tabs.removeAt(idx)
        t.group = 0
        val at = tabs.indexOfLast { it.group == old }.let { if (it >= 0) it + 1 else idx }
        tabs.add(at.coerceAtMost(tabs.size), t)
        keepActive(act)
        cleanupGroups()
        changed()
    }

    fun toggleGroup(gid: Int) {
        val g = groups.firstOrNull { it.id == gid } ?: return
        g.collapsed = !g.collapsed
        changed()
    }

    fun ungroup(gid: Int) {
        tabs.filter { it.group == gid }.forEach { it.group = 0 }
        cleanupGroups()
        changed()
    }

    fun closeGroup(gid: Int) {
        while (true) {
            val i = tabs.indexOfFirst { it.group == gid }
            if (i < 0) break
            closeTab(i)
        }
    }

    fun renameGroup(gid: Int, name: String) {
        groups.firstOrNull { it.id == gid }?.name = name.trim().filter { !it.isISOControl() }.take(40)
        changed()
    }

    fun setGroupColor(gid: Int, color: String) {
        groups.firstOrNull { it.id == gid }?.color = color
        changed()
    }

    fun groupsJson() = JSONArray().apply {
        groups.forEach { put(U.obj("id" to it.id, "name" to it.name, "color" to it.color, "collapsed" to it.collapsed)) }
    }

    // ---- changes -> bar, tab grid, session --------------------------------------------------

    private var syncPending = false
    private var savePending = false

    fun requestSync() {
        if (syncPending) return
        syncPending = true
        U.ui {
            U.later(50) {
                syncPending = false
                Bar.sync()
                Switcher.refresh()
            }
        }
    }

    fun changed() {
        requestSync()
        if (!savePending) {
            savePending = true
            U.later(2000) {
                savePending = false
                saveSession()
            }
        }
    }

    // ---- session.txt (same format as on Windows; incognito tabs are never saved) ------------

    private fun sessionFile() = File(U.dataDir(), "session.txt")

    fun saveSession() {
        val normal = tabs.filter { !it.incognito }
        val act = activeTab()
        val activeIdx = if (act != null && !act.incognito) normal.indexOf(act) else 0
        val sb = StringBuilder("${activeIdx.coerceAtLeast(0)}\n")
        normal.forEach { sb.append(it.url.replace('\n', ' ')).append('\n') }
        val used = normal.map { it.group }.toSet()
        groups.filter { it.id in used }.forEach {
            sb.append("#group\t${it.id}\t${it.color}\t${if (it.collapsed) 1 else 0}\t${it.name.replace('\t', ' ').replace('\n', ' ')}\n")
        }
        normal.forEachIndexed { i, t ->
            if (t.group != 0) sb.append("#tabgroup\t$i\t${t.group}\n")
            if (t.title.isNotEmpty()) sb.append("#tabtitle\t$i\t${t.title.replace('\t', ' ').replace('\n', ' ')}\n")
        }
        U.writeAtomic(sessionFile(), sb.toString())
        // Thumbnails of the saved tabs, so the grid has pictures after a restart.
        val dir = File(U.ctx.cacheDir, "thumbs")
        U.bg {
            dir.mkdirs()
            dir.listFiles()?.forEach { it.delete() }
            normal.forEachIndexed { i, t -> t.thumb?.let { File(dir, "$i.jpg").writeBytes(it) } }
        }
    }

    /** Start: the tabs from last time (only the selected one loads). */
    fun restore(): Boolean {
        if (!Prefs.restoreSession) return false
        val text = U.readText(sessionFile()) ?: return false
        val lines = text.lines()
        val activeIdx = lines.firstOrNull()?.trim()?.toIntOrNull() ?: 0
        val urls = lines.drop(1).map { it.trim() }.filter { it.isNotEmpty() && !it.startsWith("#") }
        if (urls.isEmpty()) return false
        val dir = File(U.ctx.cacheDir, "thumbs")
        urls.forEachIndexed { i, u ->
            val t = Tab(nextId++, u, false)
            File(dir, "$i.jpg").takeIf { it.isFile }?.let { t.thumb = it.readBytes() }
            tabs.add(t)
        }
        for (line in lines.filter { it.startsWith("#") }) {
            val p = line.split('\t')
            when (p.firstOrNull()) {
                "#group" -> if (p.size >= 4) p[1].toIntOrNull()?.let { id ->
                    groups.add(Group(id, p.drop(4).joinToString(" "), p[2], p[3] == "1"))
                    nextGroup = maxOf(nextGroup, id + 1)
                }
                "#tabgroup" -> if (p.size == 3) {
                    val i = p[1].toIntOrNull()
                    val g = p[2].toIntOrNull()
                    if (i != null && g != null) tabs.getOrNull(i)?.group = g
                }
                "#tabtitle" -> if (p.size >= 3) p[1].toIntOrNull()?.let { tabs.getOrNull(it)?.title = p.drop(2).joinToString(" ") }
            }
        }
        cleanupGroups()
        active = -1
        activate(activeIdx.coerceIn(0, tabs.size - 1))
        return true
    }
}
