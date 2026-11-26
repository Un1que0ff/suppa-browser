package com.suppa.browser

import android.content.Context
import android.content.Intent
import android.print.PrintAttributes
import android.print.PrintManager
import android.webkit.WebView
import org.json.JSONObject

/** Every command of the bar, the menus and the Settings page (window.rs command()). */
object Commands {
    const val SETTINGS_URL = "https://suppa.settings/"
    const val HISTORY_URL = "https://suppa.history/"
    const val DOWNLOADS_URL = "https://suppa.downloads/"

    /** Opens one of Suppa's pages: switches to it if it is open already. */
    fun openPage(url: String) {
        val host = U.hostOf(url)
        val i = Tabs.tabs.indexOfFirst { U.hostOf(it.url) == host && !it.incognito }
        if (i >= 0) {
            Tabs.activate(i)
            if (url.contains('#')) Tabs.tabs[i].view?.loadUrl(url)
        } else {
            Tabs.newTab(url, true, false, Tabs.active + 1)
        }
        Switcher.close()
    }

    fun run(name: String) {
        val act = MainActivity.act
        val t = Tabs.activeTab()
        val v = t?.view
        when (name) {
            "new_tab" -> { Tabs.newTab(null, true, t?.incognito ?: false); Switcher.close(); focusAddressSoon() }
            "incognito" -> { Tabs.newIncognito(); Switcher.close(); focusAddressSoon() }
            "close_tab" -> Tabs.closeTab(Tabs.active)
            "reopen" -> Tabs.reopenClosed()
            "back" -> if (v?.canGoBack() == true) v.goBack()
            "forward" -> if (v?.canGoForward() == true) v.goForward()
            "reload" -> if (v != null) v.reload() else Tabs.activate(Tabs.active)
            "home" -> Tabs.navigateActive(Prefs.home)
            "settings" -> openPage(SETTINGS_URL)
            "history" -> openPage(HISTORY_URL)
            "downloads" -> openPage(DOWNLOADS_URL)
            "bookmarks" -> Menus.bookmarks()
            "add_bookmark" -> Saved.toggleActive()
            "tabs" -> Switcher.open()
            "theme" -> PagesHost.set("dark", !Prefs.dark)
            "fullscreen" -> act.toggleFullscreen()
            "find" -> Find.open()
            "share" -> if (t != null && t.url.startsWith("http")) {
                val i = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, t.url).putExtra(Intent.EXTRA_SUBJECT, t.title)
                act.startActivity(Intent.createChooser(i, "Share"))
            }
            "desktop" -> if (t != null) Tabs.setDesktop(t, !t.desktop)
            "zoom_in" -> setZoom(Prefs.textZoom + 10)
            "zoom_out" -> setZoom(Prefs.textZoom - 10)
            "zoom_reset" -> setZoom(100)
            "print" -> if (v != null) print(v, t.title)
            "devtools" -> Popup.message(
                "Developer tools",
                "Connect the phone to a PC with a USB cable (USB debugging on), open chrome://inspect in Chrome or Edge on the PC and click \"inspect\" under this page.",
            )
            "task_manager" -> Menus.taskManager()
            "sleep_now" -> Tabs.unloadBackground()
            "update" -> act.restart()
            "restart" -> act.restart()
            "log" -> Popup.message("Log", U.readLog().lines().takeLast(40).joinToString("\n"))
            "check_update" -> Updater.check(manual = true)
            "whats_new" -> Updater.whatsNew(U.version())
            "about" -> Popup.message(
                "Suppa Browser ${U.version()}",
                "A small, fast, private browser with a built-in ad blocker, tab groups, saved tabs and your own interface.\n" +
                    "Engine: Android System WebView ${PagesHost.engineVersion()}\nhttps://github.com/Un1que0ff/suppa-browser",
            )
            "exit" -> { Tabs.saveSession(); act.finishAndRemoveTask() }
            "adblock_toggle" -> PagesHost.set("adblock", !Prefs.adblock)
            "adblock_site" -> {
                Prefs.toggleAllow(U.hostOf(t?.url ?: ""))
                Prefs.save()
                Content.invalidate()
                v?.reload()
                Bar.sync()
            }
            "update_lists" -> AdBlock.update(force = true)
            "open_scripts", "open_data" -> Popup.message(
                if (name == "open_scripts") "Scripts folder" else "Settings and data",
                if (name == "open_scripts") {
                    "Your own scripts and filter rules go in:\n${U.scriptsDir()}\n\n" +
                        "A file there (ui.css, menu.html, filters/*.txt, userscripts/*.js...) replaces the built-in one. " +
                        "Copy them there with a USB cable or a file manager, then press Update in the menu."
                } else {
                    "Settings, saved tabs, history and passwords are kept in Suppa's private storage. Use Settings -> Backup and import to copy them."
                },
            )
            "copy_scripts" -> { Scripts.copyDefaults(); run("open_scripts") }
            "backup" -> Backup.backup()
            "restore" -> Backup.restore()
            "import" -> Backup.importBookmarks()
            "import_passwords" -> Passwords.importCsv()
            "export_passwords" -> Passwords.exportCsv()
            "clear_passwords" -> Passwords.clearAll()
            "clear_data" -> PagesHost.clearData()
            "make_default" -> act.makeDefault()
            "show_dl" -> { Prefs.dlButton = !Prefs.dlButton; Prefs.save(); Downloads.syncBar() }
            "show_saved" -> PagesHost.set("saved_bar", !Prefs.showSaved)
            "store", "load_unpacked" -> Popup.message("Extensions", "Chrome extensions are not available in Suppa for Android (Android's WebView has no extension support).")
        }
    }

    private fun focusAddressSoon() {
        U.later(150) { Bar.post(U.obj("type" to "focus_address")) }
    }

    private fun setZoom(z: Int) {
        Prefs.textZoom = z.coerceIn(50, 300)
        Prefs.save()
        Tabs.applySettings()
    }

    private fun print(v: WebView, title: String) {
        val pm = MainActivity.act.getSystemService(Context.PRINT_SERVICE) as PrintManager
        val name = title.ifEmpty { "Suppa page" }
        pm.print(name, v.createPrintDocumentAdapter(name), PrintAttributes.Builder().build())
    }
}

/** Find in page: a row over the address bar (mobile.js), WebView's own search. */
object Find {
    private var open = false

    fun open() {
        open = true
        Bar.post(U.obj("type" to "find", "open" to true))
        Tabs.activeView()?.setFindListener { i, n, done ->
            if (done) Bar.post(U.obj("type" to "find_result", "i" to (if (n == 0) 0 else i + 1), "n" to n))
        }
    }

    fun close() {
        if (!open) return
        open = false
        Tabs.activeView()?.clearMatches()
        Bar.post(U.obj("type" to "find", "open" to false))
    }

    fun isOpen() = open

    fun onMessage(v: JSONObject) {
        val view = Tabs.activeView() ?: return
        when (v.optString("op")) {
            "text" -> view.findAllAsync(v.optString("text"))
            "next" -> view.findNext(true)
            "prev" -> view.findNext(false)
            "close" -> close()
        }
    }
}

/** The user's scripts folder (the built-in files are inside the app). */
object Scripts {
    private val DEFAULTS = listOf(
        "ui.html", "ui.css", "ui.js", "mobile.css", "mobile.js", "menu.html", "tabs.html", "settings.html",
        "newtab.html", "history.html", "downloads.html", "cookies.js", "youtube.js", "page.js", "README.txt",
        "filters/my-filters.txt", "userscripts/_example.js",
    )

    /** Copies the built-in files to the scripts folder so they can be edited. */
    fun copyDefaults() {
        val dir = U.scriptsDir()
        for (name in DEFAULTS) {
            val f = java.io.File(dir, name)
            if (f.exists()) continue
            val bytes = try { U.ctx.assets.open(name).use { it.readBytes() } } catch (_: Exception) { continue }
            f.parentFile?.mkdirs()
            f.writeBytes(bytes)
        }
    }

    private var observer: android.os.FileObserver? = null

    /** A file in the scripts folder changed: the ≡ button becomes "Update ≡". */
    fun watch() {
        val dir = U.scriptsDir()
        dir.mkdirs()
        java.io.File(dir, "filters").mkdirs()
        java.io.File(dir, "userscripts").mkdirs()
        val mask = android.os.FileObserver.CLOSE_WRITE or android.os.FileObserver.MOVED_TO or android.os.FileObserver.DELETE or android.os.FileObserver.CREATE
        observer = object : android.os.FileObserver(listOf(dir, java.io.File(dir, "filters"), java.io.File(dir, "userscripts")), mask) {
            override fun onEvent(event: Int, path: String?) {
                U.ui {
                    if (!Bar.updateAvailable) {
                        U.log("scripts folder changed - Update button lit")
                        Bar.updateAvailable = true
                        Bar.postUpdate()
                    }
                }
            }
        }.also { it.startWatching() }
    }
}
