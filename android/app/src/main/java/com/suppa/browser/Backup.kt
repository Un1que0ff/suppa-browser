package com.suppa.browser

import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.zip.ZipEntry
import java.util.zip.ZipInputStream
import java.util.zip.ZipOutputStream

/**
 * Backup and restore (backup.rs): one .zip with settings, saved tabs, open
 * tabs, history, downloads list, your scripts and the saved passwords (they
 * stay encrypted for this phone, so they restore on this phone only).
 * Plus bookmark import from an exported bookmarks .html file (import.rs).
 */
object Backup {
    private val FILES = listOf("settings.ini", "saved.json", "session.txt", "downloads.json", "passwords.json", "passwords_never.txt", "newtab_hidden.txt")

    fun backup() {
        val name = "Suppa backup " + SimpleDateFormat("yyyy-MM-dd", Locale.ROOT).format(Date()) + ".zip"
        Tabs.saveSession()
        MainActivity.act.createDocument(name, "application/zip") { uri ->
            uri ?: return@createDocument
            U.bg {
                var n = 0
                U.ctx.contentResolver.openOutputStream(uri)?.use { os ->
                    ZipOutputStream(os).use { zip ->
                        fun add(entry: String, f: File) {
                            if (!f.isFile) return
                            zip.putNextEntry(ZipEntry(entry))
                            f.inputStream().use { it.copyTo(zip) }
                            zip.closeEntry()
                            n++
                        }
                        FILES.forEach { add(it, File(U.dataDir(), it)) }
                        History.dbFile().let { add("history.db", it) }
                        U.scriptsDir().walkTopDown().filter { it.isFile }.forEach {
                            add("scripts/" + it.relativeTo(U.scriptsDir()).path.replace('\\', '/'), it)
                        }
                    }
                }
                U.ui { Popup.message("Backup ready", "$n files saved in $name.") }
            }
        }
    }

    fun restore() {
        MainActivity.act.openDocument(arrayOf("application/zip", "application/octet-stream")) { uri ->
            uri ?: return@openDocument
            Popup.confirm(
                "Restore this backup?",
                "Your settings, saved tabs, open tabs, history and saved passwords are replaced by the backup's, then Suppa restarts.",
                "Restore",
            ) {
                U.bg {
                    var n = 0
                    History.closeDb()
                    U.ctx.contentResolver.openInputStream(uri)?.use { ins ->
                        ZipInputStream(ins).use { zip ->
                            while (true) {
                                val e = zip.nextEntry ?: break
                                val target = when {
                                    e.name in FILES -> File(U.dataDir(), e.name)
                                    e.name == "history.db" -> History.dbFile()
                                    e.name.startsWith("scripts/") && !e.name.contains("..") && !e.isDirectory -> File(U.scriptsDir(), e.name.removePrefix("scripts/"))
                                    else -> null
                                } ?: continue
                                target.parentFile?.mkdirs()
                                target.outputStream().use { zip.copyTo(it) }
                                n++
                            }
                        }
                    }
                    U.log("backup restored: $n files")
                    U.ui { MainActivity.act.restart(saveSession = false) }
                }
            }
        }
    }

    /** Bookmarks exported from Chrome, Firefox, Edge... (Netscape bookmarks .html) -> saved tabs. */
    fun importBookmarks() {
        MainActivity.act.openDocument(arrayOf("text/html", "text/*", "application/octet-stream")) { uri ->
            uri ?: return@openDocument
            val html = try { U.ctx.contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() } } catch (_: Exception) { null } ?: return@openDocument
            val nodes = mutableListOf<Node>()
            val stack = ArrayDeque<String>()
            var pendingFolder: String? = null
            val folders = LinkedHashMap<String, Node>()
            val tag = Regex("""<(/?)(DL|H3|A)\b([^>]*)>(?:([^<]*))?""", RegexOption.IGNORE_CASE)
            for (m in tag.findAll(html)) {
                val close = m.groupValues[1] == "/"
                val name = m.groupValues[2].uppercase()
                when {
                    name == "H3" && !close -> pendingFolder = unescape(m.groupValues[4])
                    name == "DL" && !close -> { stack.addLast(pendingFolder ?: ""); pendingFolder = null }
                    name == "DL" && close -> stack.removeLastOrNull()
                    name == "A" && !close -> {
                        val href = Regex("""HREF="([^"]*)"""", RegexOption.IGNORE_CASE).find(m.groupValues[3])?.groupValues?.get(1) ?: continue
                        if (!href.startsWith("http")) continue
                        val icon = Regex("""ICON_URI="([^"]*)"""", RegexOption.IGNORE_CASE).find(m.groupValues[3])?.groupValues?.get(1) ?: ""
                        val link = Link(unescape(m.groupValues[4]).trim(), unescape(href), if (icon.startsWith("http")) icon else "")
                        // Saved tabs have one level of folders: the innermost named folder.
                        val folder = stack.lastOrNull { it.isNotEmpty() && it != "Bookmarks bar" && it != "Bookmarks Toolbar" && it != "Favorites bar" }
                        if (folder == null) nodes.add(Node(link = link))
                        else folders.getOrPut(folder) { Node(folder = folder).also { nodes.add(it) } }.items.add(link)
                    }
                }
            }
            val n = Saved.addImported(nodes)
            Popup.message("Import bookmarks", if (n == 0) "No new bookmarks found in this file." else "$n bookmarks imported. They are in ≡ -> Bookmarks.")
        }
    }

    private fun unescape(s: String) = s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}
