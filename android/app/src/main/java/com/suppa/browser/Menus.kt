package com.suppa.browser

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.webkit.WebView

/** The ≡ menu, Bookmarks, ad blocker, tab, group and page menus (menu.rs). */
object Menus {
    /** Where the last ≡ menu hung from (Bookmarks opens at the same place). */
    private var lastX = 0f
    private var lastY = 0f

    private fun stageW() = MainActivity.act.stage.width.toFloat()

    fun main(x: Float, y: Float) {
        lastX = x
        lastY = y
        val t = Tabs.activeTab()
        val v = t?.view
        val upd = if (Bar.updateAvailable) "Update  ●  changes ready" else "Update (restart and reload scripts)"
        val items = mutableListOf(
            Item(1, "Forward").enabled(v?.canGoForward() == true),
            Item(2, "Reload"),
            Item.sep(),
            Item(3, "New tab"),
            Item(4, "New incognito tab"),
            Item(5, "Reopen closed tab").enabled(Tabs.hasClosed()),
            Item(6, "Tabs (${Tabs.tabs.size})"),
            Item.sep(),
            Item(7, "Bookmarks"),
            Item(8, "History"),
            Item(9, "Downloads"),
            Item.sep(),
            Item(10, "Find in page").enabled(v != null),
            Item(11, "Share...").enabled(t?.url?.startsWith("http") == true),
            Item(12, "Desktop site").checked(t?.desktop == true),
            Item.sub(
                "Page",
                listOf(
                    Item(30, "Text larger").key("${Prefs.textZoom}%"),
                    Item(31, "Text smaller"),
                    Item(32, "Text normal size"),
                    Item.sep(),
                    Item(33, "Print..."),
                    Item(34, "Developer tools"),
                ),
            ),
            Item.sub("Ad blocker - ${t?.blocked ?: 0} blocked", adblockItems()),
            Item(13, "Full screen"),
            Item(14, "Task manager"),
            Item(15, "Sleep background tabs now"),
            Item.sep(),
            Item(16, if (Prefs.dark) "Light theme" else "Dark theme"),
            Item(17, "Show download button").checked(Prefs.dlButton),
            Item.sep(),
            Item(18, "Settings"),
            Item(19, "Check for updates"),
            Item.sub(
                "Backup and import",
                listOf(
                    Item(40, "Back up..."),
                    Item(41, "Restore from a backup..."),
                    Item.sep(),
                    Item(42, "Import bookmarks (.html file)..."),
                    Item(43, "Import passwords (.csv file)..."),
                    Item(44, "Export passwords..."),
                ),
            ),
            Item(20, upd).hot(Bar.updateAvailable),
            Item(21, "Log"),
            Item(22, "About Suppa"),
            Item.sep(),
            Item(23, "Exit"),
        )
        Popup.show("main", items, x, y) { id ->
            when (id?.toInt()) {
                1 -> Commands.run("forward")
                2 -> Commands.run("reload")
                3 -> Commands.run("new_tab")
                4 -> Commands.run("incognito")
                5 -> Commands.run("reopen")
                6 -> Commands.run("tabs")
                7 -> U.later(30) { bookmarks() }
                8 -> Commands.run("history")
                9 -> Commands.run("downloads")
                10 -> Commands.run("find")
                11 -> Commands.run("share")
                12 -> Commands.run("desktop")
                13 -> Commands.run("fullscreen")
                14 -> U.later(30) { taskManager() }
                15 -> Commands.run("sleep_now")
                16 -> Commands.run("theme")
                17 -> Commands.run("show_dl")
                18 -> Commands.run("settings")
                19 -> Commands.run("check_update")
                20 -> Commands.run("update")
                21 -> Commands.run("log")
                22 -> Commands.run("about")
                23 -> Commands.run("exit")
                30 -> Commands.run("zoom_in")
                31 -> Commands.run("zoom_out")
                32 -> Commands.run("zoom_reset")
                33 -> Commands.run("print")
                34 -> Commands.run("devtools")
                40 -> Commands.run("backup")
                41 -> Commands.run("restore")
                42 -> Commands.run("import")
                43 -> Commands.run("import_passwords")
                44 -> Commands.run("export_passwords")
                else -> runAdblock(id)
            }
        }
    }

    fun adblockItems(): List<Item> {
        val t = Tabs.activeTab()
        val host = U.hostOf(t?.url ?: "")
        val status = if (!AdBlock.ready) "Filter lists loading..." else "${t?.blocked ?: 0} ads/trackers blocked on this page"
        val items = mutableListOf(
            Item(0, status).enabled(false),
            Item.sep(),
            Item(50, "Ad blocker enabled").checked(Prefs.adblock),
        )
        if (host.isNotEmpty() && !host.startsWith("suppa.")) items.add(Item(51, "Allow ads on ${host.removePrefix("www.")}").checked(Prefs.isAllowed(host)))
        items.add(Item.sep())
        items.add(Item(52, "Update filter lists now"))
        items.add(Item(53, "My filter rules (scripts/filters)"))
        return items
    }

    private fun runAdblock(id: Long?) {
        when (id?.toInt()) {
            50 -> Commands.run("adblock_toggle")
            51 -> Commands.run("adblock_site")
            52 -> Commands.run("update_lists")
            53 -> Commands.run("open_scripts")
        }
    }

    fun adblock(x: Float, y: Float) = Popup.show("adblock", adblockItems(), x, y) { runAdblock(it) }

    // ---- Bookmarks: their own popup from the ≡ menu ----------------------------------------

    private const val LINK = 1000L
    private const val L_NEW = 2000L
    private const val L_INC = 3000L
    private const val L_REN = 4000L
    private const val L_DEL = 5000L
    private const val F_ALL = 6000L
    private const val IN = 100_000L          // + folder * 1000 + j
    private const val IN_NEW = 1_000_000L
    private const val IN_INC = 2_000_000L
    private const val IN_REN = 3_000_000L
    private const val IN_DEL = 4_000_000L
    private const val IN_OUT = 5_000_000L

    private fun bookmarkItems(): List<Item> {
        val items = mutableListOf(Item(0, "Bookmarks").enabled(false), Item.sep())
        val nodes = Saved.nodes()
        if (nodes.isEmpty()) items.add(Item(0, "No bookmarks yet - tap ☆ in the address bar").enabled(false))
        nodes.forEachIndexed { i, n ->
            val f = n.folder
            if (f == null) {
                val l = n.link!!
                items.add(
                    Item(LINK + i, Saved.displayTitle(l)).icon(Saved.iconOf(l)).draggable().ctx(
                        listOf(
                            Item(L_NEW + i, "Open in new tab"), Item(L_INC + i, "Open in incognito tab"), Item.sep(),
                            Item(L_REN + i, "Rename"), Item(L_DEL + i, "Delete"),
                        ),
                    ),
                )
            } else {
                val sub = mutableListOf<Item>()
                n.items.forEachIndexed { j, l ->
                    val p = i * 1000L + j
                    sub.add(
                        Item(IN + p, Saved.displayTitle(l)).icon(Saved.iconOf(l)).ctx(
                            listOf(
                                Item(IN_NEW + p, "Open in new tab"), Item(IN_INC + p, "Open in incognito tab"), Item.sep(),
                                Item(IN_REN + p, "Rename"), Item(IN_OUT + p, "Move out of folder"), Item(IN_DEL + p, "Delete"),
                            ),
                        ),
                    )
                }
                if (sub.isEmpty()) sub.add(Item(0, "Empty folder").enabled(false)) else {
                    sub.add(Item.sep())
                    sub.add(Item(F_ALL + i, "Open all in new tabs"))
                }
                items.add(
                    Item.sub("$f (${n.items.size})", sub).apply { drag = true }.ctx(
                        listOf(Item(F_ALL + i, "Open all in new tabs"), Item.sep(), Item(L_REN + i, "Rename folder"), Item(L_DEL + i, "Delete folder")),
                    ),
                )
            }
        }
        val url = Tabs.activeTab()?.url ?: ""
        val canSave = url.startsWith("http") && Tabs.activeTab()?.incognito == false
        items.add(Item.sep())
        items.add(Item(10, if (Saved.contains(url)) "Remove this page from bookmarks" else "Bookmark this page").enabled(canSave))
        items.add(Item(11, "New folder"))
        items.add(Item(12, "Show bookmarks bar").checked(Prefs.showSaved))
        return items
    }

    fun bookmarks() {
        val x = if (lastX > 0) lastX else stageW() - U.dp(8f)
        val y = if (lastY > 0) lastY else U.dp(56f).toFloat()
        Popup.show("bookmarks", bookmarkItems(), x, y, reorder = { from, to ->
            Saved.reorderTop(from, to)
            Popup.refresh(bookmarkItems())
        }) { id -> if (id != null) pickBookmark(id) }
    }

    private fun pickBookmark(id: Long) {
        fun link(i: Int, j: Int = -1) = Saved.getLink(if (j < 0) listOf(i) else listOf(i, j))
        when {
            id == 10L -> Saved.toggleActive()
            id == 11L -> Popup.prompt("New folder", "New folder") { name -> if (!name.isNullOrBlank()) Saved.newFolder(name.trim()) }
            id == 12L -> Commands.run("show_saved")
            id >= IN_OUT -> { val p = id - IN_OUT; Saved.moveItem(listOf((p / 1000).toInt(), (p % 1000).toInt()), "before", (p / 1000).toInt() + 1) }
            id >= IN_DEL -> { val p = id - IN_DEL; Saved.delete(listOf((p / 1000).toInt(), (p % 1000).toInt())) }
            id >= IN_REN -> { val p = id - IN_REN; rename(listOf((p / 1000).toInt(), (p % 1000).toInt())) }
            id >= IN_INC -> { val p = id - IN_INC; link((p / 1000).toInt(), (p % 1000).toInt())?.let { Tabs.openUrl(it.url, "inc") } }
            id >= IN_NEW -> { val p = id - IN_NEW; link((p / 1000).toInt(), (p % 1000).toInt())?.let { Tabs.openUrl(it.url, "new") } }
            id >= IN -> { val p = id - IN; link((p / 1000).toInt(), (p % 1000).toInt())?.let { Tabs.openUrl(it.url, "same") } }
            id >= F_ALL -> Saved.openAll((id - F_ALL).toInt())
            id >= L_DEL -> Saved.delete(listOf((id - L_DEL).toInt()))
            id >= L_REN -> rename(listOf((id - L_REN).toInt()))
            id >= L_INC -> link((id - L_INC).toInt())?.let { Tabs.openUrl(it.url, "inc") }
            id >= L_NEW -> link((id - L_NEW).toInt())?.let { Tabs.openUrl(it.url, "new") }
            id >= LINK -> link((id - LINK).toInt())?.let { Tabs.openUrl(it.url, "same") }
        }
        Switcher.close()
    }

    private fun rename(path: List<Int>) {
        Popup.prompt("Rename", Saved.titleAt(path)) { v -> if (!v.isNullOrBlank()) Saved.rename(path, v.trim()) }
    }

    // ---- tabs ------------------------------------------------------------------------------

    /** Long press on a tab (in the tab grid) or the bar's tab menu. */
    fun tab(index: Int, x: Float, y: Float, right: Boolean = false) {
        val t = Tabs.tabs.getOrNull(index) ?: return
        val saved = Saved.contains(t.url)
        val grouped = t.group != 0
        val items = listOf(
            Item(1, if (saved) "Saved ✓" else "Save to bookmarks").enabled(!saved && !t.incognito && t.url.startsWith("http")),
            Item(11, "Share...").enabled(t.url.startsWith("http")),
            Item.sep(),
            Item(2, "New tab"),
            Item(3, "Reload"),
            Item(4, "Duplicate"),
            Item.sep(),
            Item(5, "Close tab"),
            Item(6, "Close other tabs").enabled(Tabs.tabs.count { it.incognito == t.incognito } > 1),
            Item(7, "Reopen closed tab").enabled(Tabs.hasClosed()),
            Item.sep(),
            if (grouped) Item(9, "Remove from group") else Item(8, "Add to new group"),
            Item(10, "Ungroup (whole group)").enabled(grouped),
        )
        Popup.show("tab", items, x, y, right) { id ->
            val i = Tabs.indexOf(t)
            if (i < 0) return@show
            when (id?.toInt()) {
                1 -> Saved.saveTab(t)
                2 -> Commands.run("new_tab")
                3 -> Tabs.reload(i)
                4 -> Tabs.duplicate(i)
                5 -> Tabs.closeTab(i)
                6 -> Tabs.closeOthers(i)
                7 -> Tabs.reopenClosed()
                8 -> Switcher.startRename(Tabs.addToNewGroup(i))
                9 -> Tabs.removeFromGroup(i)
                10 -> Tabs.ungroup(t.group)
                11 -> {
                    val s = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, t.url)
                    MainActivity.act.startActivity(Intent.createChooser(s, "Share"))
                }
            }
        }
    }

    fun group(gid: Int, x: Float, y: Float) {
        val g = Tabs.groups.firstOrNull { it.id == gid } ?: return
        val n = Tabs.tabs.count { it.group == gid }
        val title = (g.name.ifEmpty { "Group" }) + " ($n tabs)"
        val colors = Tabs.GROUP_COLORS.mapIndexed { i, c -> Item(100L + i, c.first).checked(g.color == c.second) }
        val items = listOf(
            Item(0, title).enabled(false), Item.sep(),
            Item(1, "Rename"), Item.sub("Colour", colors), Item(2, if (g.collapsed) "Expand" else "Collapse"),
            Item.sep(), Item(3, "Ungroup"), Item(4, "Close group"),
        )
        Popup.show("group", items, x, y, false) { id ->
            when (val k = id?.toInt()) {
                1 -> Switcher.startRename(gid)
                2 -> Tabs.toggleGroup(gid)
                3 -> Tabs.ungroup(gid)
                4 -> Tabs.closeGroup(gid)
                null -> {}
                else -> if (k in 100..107) Tabs.setGroupColor(gid, Tabs.GROUP_COLORS[k - 100].second)
            }
        }
    }

    /** Long press on the tabs button. */
    fun tabsButton(x: Float, y: Float) {
        val items = listOf(Item(1, "New tab"), Item(2, "New incognito tab"), Item.sep(), Item(3, "Close tab"))
        Popup.show("tabsbtn", items, x, y) { id ->
            when (id?.toInt()) {
                1 -> Commands.run("new_tab")
                2 -> Commands.run("incognito")
                3 -> Commands.run("close_tab")
            }
        }
    }

    /** The ≡ button of the tab grid. */
    fun switcherMenu(x: Float, y: Float, incognito: Boolean) {
        val items = listOf(
            Item(1, "New tab"), Item(2, "New incognito tab"), Item.sep(),
            Item(3, if (incognito) "Close all incognito tabs" else "Close all tabs"),
            Item(4, "Reopen closed tab").enabled(Tabs.hasClosed()),
            Item.sep(), Item(5, "Settings"),
        )
        Popup.show("switcher", items, x, y) { id ->
            when (id?.toInt()) {
                1 -> Commands.run("new_tab")
                2 -> Commands.run("incognito")
                3 -> Popup.confirm(if (incognito) "Close all incognito tabs?" else "Close all tabs?", "", "Close all") { Tabs.closeAll(incognito) }
                4 -> Tabs.reopenClosed()
                5 -> Commands.run("settings")
            }
        }
    }

    fun taskManager() {
        val items = mutableListOf(Item(0, "Tabs - tap a loaded tab to unload it").enabled(false), Item.sep())
        Tabs.tabs.forEachIndexed { i, t ->
            val state = when {
                i == Tabs.active -> "showing"
                t.view != null -> "loaded"
                else -> "asleep"
            }
            items.add(Item(100L + i, Bar.tabTitle(t)).key(state).enabled(t.view != null && i != Tabs.active))
        }
        items.add(Item.sep())
        items.add(Item(1, "Sleep all background tabs"))
        Popup.show("tasks", items, stageW() / 2, U.dp(80f).toFloat(), center = true) { id ->
            val k = id?.toInt() ?: return@show
            if (k == 1) Tabs.unloadBackground() else Tabs.tabs.getOrNull(k - 100)?.let { Tabs.unload(it) }
        }
    }

    // ---- long press on a page --------------------------------------------------------------

    fun pageContext(t: Tab, v: WebView, x: Float, y: Float): Boolean {
        val hit = v.hitTestResult
        val extra = hit.extra ?: return false
        val type = hit.type
        val loc = IntArray(2)
        v.getLocationInWindow(loc)
        val st = IntArray(2)
        MainActivity.act.stage.getLocationInWindow(st)
        val sx = x + loc[0] - st[0] - MainActivity.act.stage.paddingLeft
        val sy = y + loc[1] - st[1] - MainActivity.act.stage.paddingTop
        when (type) {
            WebView.HitTestResult.SRC_ANCHOR_TYPE -> linkMenu(t, extra, null, sx, sy)
            WebView.HitTestResult.IMAGE_TYPE -> linkMenu(t, null, extra, sx, sy)
            WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE -> {
                val h = Handler(Looper.getMainLooper()) { m ->
                    linkMenu(t, m.data.getString("url"), extra, sx, sy)
                    true
                }
                v.requestFocusNodeHref(h.obtainMessage())
            }
            else -> return false
        }
        return true
    }

    private fun linkMenu(t: Tab, link: String?, image: String?, x: Float, y: Float) {
        val items = mutableListOf<Item>()
        if (link != null) {
            items.add(Item(0, link.take(80)).enabled(false))
            items.add(Item.sep())
            items.add(Item(1, "Open in new tab"))
            items.add(Item(2, "Open in background tab"))
            items.add(Item(3, "Open in incognito tab"))
            items.add(Item.sep())
            items.add(Item(4, "Copy link address"))
            items.add(Item(5, "Share link"))
            items.add(Item(6, "Download link"))
            items.add(Item(7, "Save link to bookmarks").enabled(!Saved.contains(link) && link.startsWith("http")))
        }
        if (image != null) {
            if (items.isNotEmpty()) items.add(Item.sep())
            items.add(Item(10, "Open image in new tab"))
            items.add(Item(11, "Download image"))
            items.add(Item(12, "Copy image address"))
        }
        Popup.show("page", items, x, y, right = x > stageW() / 2) { id ->
            when (id?.toInt()) {
                1 -> Tabs.newTab(link, true, t.incognito, Tabs.indexOf(t) + 1, t)
                2 -> Tabs.newTab(link, false, t.incognito, Tabs.indexOf(t) + 1, t)
                3 -> Tabs.newIncognito(link)
                4 -> copy(link ?: "")
                5 -> MainActivity.act.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, link), "Share"))
                6 -> link?.let { Downloads.start(t, it, t.view?.settings?.userAgentString ?: "", "", "") }
                7 -> link?.let { Saved.add(Link(U.siteOf(it), it, "")) }
                10 -> Tabs.newTab(image, true, t.incognito, Tabs.indexOf(t) + 1, t)
                11 -> image?.let { Downloads.start(t, it, t.view?.settings?.userAgentString ?: "", "", "") }
                12 -> copy(image ?: "")
            }
        }
    }

    fun copy(text: String) {
        val cm = MainActivity.act.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        cm.setPrimaryClip(ClipData.newPlainText("Suppa", text))
    }
}
