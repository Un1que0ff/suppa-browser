package com.suppa.browser

import org.json.JSONArray
import org.json.JSONObject
import java.io.File

class Link(var title: String, var url: String, var icon: String)

/** A saved link, or a folder of links (folder != null). */
class Node(val link: Link? = null, var folder: String? = null, val items: MutableList<Link> = mutableListOf())

/**
 * Saved tabs (bookmarks), saved.json in the same format as on Windows (saved.rs).
 * On a phone they live in ≡ -> Bookmarks (the bar row can be turned on in Settings).
 */
object Saved {
    private var list: MutableList<Node>? = null
    private fun file() = File(U.dataDir(), "saved.json")

    fun nodes(): MutableList<Node> = list ?: load().also { list = it }

    private fun linkFrom(o: JSONObject): Link? {
        val url = o.optString("url")
        if (url.isEmpty()) return null
        return Link(o.optString("title"), url, o.optString("icon"))
    }

    private fun load(): MutableList<Node> {
        val text = U.readText(file())?.trimStart('﻿') ?: return mutableListOf()
        val arr = try { JSONArray(text) } catch (_: Exception) { return mutableListOf() }
        val out = mutableListOf<Node>()
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            if (o.has("folder")) {
                val items = mutableListOf<Link>()
                o.optJSONArray("items")?.let { a -> for (j in 0 until a.length()) a.optJSONObject(j)?.let { linkFrom(it) }?.let { items.add(it) } }
                out.add(Node(folder = o.optString("folder"), items = items))
            } else {
                linkFrom(o)?.let { out.add(Node(link = it)) }
            }
        }
        return out
    }

    private fun linkJson(l: Link) = U.obj("title" to l.title, "url" to l.url, "icon" to l.icon)

    private fun save() {
        val arr = JSONArray()
        for (n in nodes()) {
            if (n.folder != null) arr.put(U.obj("folder" to n.folder, "items" to JSONArray().apply { n.items.forEach { put(linkJson(it)) } }))
            else arr.put(linkJson(n.link!!))
        }
        U.writeAtomic(file(), arr.toString(2))
    }

    fun displayTitle(l: Link) = l.title.trim().ifEmpty { U.siteOf(l.url) }
    fun iconOf(l: Link) = l.icon.ifEmpty { History.iconFor(l.url) ?: "" }

    /** The saved items to the bar (when its row is shown) and the star. */
    fun sync() {
        val items = JSONArray()
        for (n in nodes()) {
            if (n.folder != null) items.put(U.obj("f" to n.folder, "n" to n.items.size))
            else n.link!!.let { items.put(U.obj("t" to displayTitle(it), "u" to it.url, "i" to iconOf(it))) }
        }
        Bar.post(U.obj("type" to "saved", "items" to items))
        Tabs.requestSync()
    }

    fun contains(url: String) = url.isNotEmpty() && nodes().any { n -> n.link?.url == url || n.items.any { it.url == url } }

    fun add(l: Link) {
        if (contains(l.url)) return
        nodes().add(Node(link = l))
        save()
        sync()
    }

    fun saveTab(t: Tab) {
        if (t.url.isEmpty() || t.incognito || !t.url.startsWith("http")) return
        add(Link(t.title.ifEmpty { U.siteOf(t.url) }, t.url, if (t.favicon.startsWith("http")) t.favicon else ""))
    }

    /** ☆ in the address bar: save this page, or remove it when it is saved. */
    fun toggleActive() {
        val t = Tabs.activeTab() ?: return
        val url = t.url
        if (url.isEmpty()) return
        if (contains(url)) {
            nodes().removeAll { it.link?.url == url }
            nodes().forEach { n -> n.items.removeAll { it.url == url } }
            save()
            sync()
        } else {
            saveTab(t)
        }
    }

    fun learnIcon(pageUrl: String, icon: String) {
        if (!icon.startsWith("http") || pageUrl.isEmpty()) return
        val host = U.siteOf(pageUrl)
        var changed = false
        fun fix(l: Link) {
            val same = l.url.trimEnd('/') == pageUrl.trimEnd('/')
            if (l.icon != icon && (same || (l.icon.isEmpty() && host.isNotEmpty() && U.siteOf(l.url) == host))) {
                l.icon = icon
                changed = true
            }
        }
        for (n in nodes()) {
            n.link?.let { fix(it) }
            n.items.forEach { fix(it) }
        }
        if (changed) {
            save()
            sync()
        }
    }

    fun getLink(path: List<Int>): Link? {
        val n = nodes().getOrNull(path.getOrNull(0) ?: return null) ?: return null
        return if (path.size == 1) n.link else n.items.getOrNull(path[1])
    }

    fun titleAt(path: List<Int>): String {
        val n = nodes().getOrNull(path.getOrNull(0) ?: return "") ?: return ""
        return if (path.size == 1) n.folder ?: n.link?.title ?: "" else n.items.getOrNull(path[1])?.title ?: ""
    }

    private fun take(path: List<Int>): Node? {
        val v = nodes()
        return when (path.size) {
            1 -> if (path[0] in v.indices) v.removeAt(path[0]) else null
            2 -> v.getOrNull(path[0])?.takeIf { it.folder != null && path[1] in it.items.indices }?.let { Node(link = it.items.removeAt(path[1])) }
            else -> null
        }
    }

    fun rename(path: List<Int>, title: String) {
        val n = nodes().getOrNull(path.getOrNull(0) ?: return) ?: return
        if (path.size == 1) {
            if (n.folder != null) n.folder = title else n.link?.title = title
        } else {
            n.items.getOrNull(path[1])?.title = title
        }
        save()
        sync()
    }

    fun delete(path: List<Int>) {
        take(path)
        save()
        sync()
    }

    fun newFolder(name: String, at: Int = Int.MAX_VALUE): Int {
        val v = nodes()
        val i = at.coerceIn(0, v.size)
        v.add(i, Node(folder = name))
        save()
        sync()
        return i
    }

    fun openAll(fi: Int) {
        val links = nodes().getOrNull(fi)?.items?.toList() ?: return
        links.forEachIndexed { k, l -> Tabs.newTab(l.url, k == 0, false, Tabs.active + 1 + k, Tabs.activeTab()) }
    }

    fun reorderTop(from: Int, to: Int) {
        val v = nodes()
        if (from !in v.indices || to !in v.indices) return
        v.add(to, v.removeAt(from))
        save()
        sync()
    }

    /** Drag & drop (saved.rs move_item): "before" = to top-level position, "into" a folder, "merge" = new folder. */
    fun moveItem(from: List<Int>, kind: String, target0: Int) {
        val v = nodes()
        var target = target0
        if (from.size == 1 && kind != "before" && from[0] == target) return
        if (from.size == 1 && from[0] < target) target--
        val node = take(from) ?: return
        val l = node.link
        when {
            kind == "into" && l != null && v.getOrNull(target)?.folder != null -> v[target].items.add(l)
            kind == "merge" && l != null && v.getOrNull(target)?.link != null -> {
                val first = v.removeAt(target).link!!
                v.add(target, Node(folder = "New folder", items = mutableListOf(first, l)))
            }
            kind == "merge" && l != null && v.getOrNull(target)?.folder != null -> v[target].items.add(l)
            else -> v.add(target.coerceIn(0, v.size), node)
        }
        save()
        sync()
    }

    /** Imported bookmarks (skips links already saved). Returns how many were added. */
    fun addImported(nodes: List<Node>): Int {
        val v = nodes()
        val known = v.flatMap { n -> listOfNotNull(n.link?.url) + n.items.map { it.url } }.toMutableSet()
        var added = 0
        for (n in nodes) {
            if (n.folder == null) {
                val l = n.link ?: continue
                if (known.add(l.url)) { v.add(n); added++ }
            } else {
                val fresh = n.items.filter { known.add(it.url) }
                if (fresh.isEmpty()) continue
                added += fresh.size
                val existing = v.firstOrNull { it.folder == n.folder }
                if (existing != null) existing.items.addAll(fresh) else v.add(Node(folder = n.folder, items = fresh.toMutableList()))
            }
        }
        save()
        sync()
        return added
    }

    /** Messages from the bookmarks row of the bar (ui.js), when it is turned on. */
    fun onBarMessage(cmd: String, v: JSONObject, x: Float, y: Float) {
        val path = v.optJSONArray("path")?.let { a -> (0 until a.length()).map { a.optInt(it) } } ?: emptyList()
        when (cmd) {
            "saved_open" -> getLink(path)?.let { Tabs.openUrl(it.url, v.optString("how", "same")) }
            "saved_folder" -> Menus.bookmarks()
            "saved_menu" -> Menus.bookmarks()
            "saved_move" -> moveItem(path, v.optString("kind", "before"), v.optInt("target"))
            "saved_rename" -> {
                val t = v.optString("title").trim()
                if (t.isNotEmpty()) rename(path, t) else sync()
            }
        }
    }
}
