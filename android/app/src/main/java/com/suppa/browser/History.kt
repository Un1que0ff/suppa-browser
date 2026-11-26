package com.suppa.browser

import android.content.ContentValues
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import org.json.JSONArray
import java.io.File

/** Every page opened in a normal tab (history.rs): the History page and the start page shortcuts. */
object History {
    private class Db : SQLiteOpenHelper(U.ctx, "history.db", null, 1) {
        override fun onCreate(db: SQLiteDatabase) {
            db.execSQL("CREATE TABLE visits(id INTEGER PRIMARY KEY, t INTEGER, url TEXT, title TEXT, host TEXT)")
            db.execSQL("CREATE INDEX visits_t ON visits(t)")
            db.execSQL("CREATE INDEX visits_url ON visits(url)")
            db.execSQL("CREATE TABLE icons(host TEXT PRIMARY KEY, icon TEXT)")
        }

        override fun onUpgrade(db: SQLiteDatabase, a: Int, b: Int) {}
    }

    private val db by lazy { Db().writableDatabase }
    private val hidden by lazy {
        (U.readText(File(U.dataDir(), "newtab_hidden.txt")) ?: "").lines().filter { it.isNotBlank() }.toMutableSet()
    }

    fun visit(url: String, title: String) {
        val host = U.siteOf(url)
        if (host.isEmpty() || host.startsWith("suppa.")) return
        val now = System.currentTimeMillis()
        U.bg {
            // The same page again within half an hour: one entry.
            val c = db.rawQuery("SELECT id FROM visits WHERE url=? AND t>? ORDER BY t DESC LIMIT 1", arrayOf(url, (now - 1_800_000).toString()))
            val id = if (c.moveToFirst()) c.getLong(0) else -1L
            c.close()
            val cv = ContentValues().apply {
                put("t", now)
                put("url", url)
                if (title.isNotEmpty()) put("title", title)
                put("host", host)
            }
            if (id >= 0) db.update("visits", cv, "id=?", arrayOf(id.toString())) else db.insert("visits", null, cv)
        }
    }

    fun setTitle(url: String, title: String) {
        if (title.isEmpty()) return
        U.bg {
            db.execSQL(
                "UPDATE visits SET title=? WHERE id=(SELECT id FROM visits WHERE url=? ORDER BY t DESC LIMIT 1)",
                arrayOf(title, url),
            )
        }
    }

    private val iconCache = java.util.concurrent.ConcurrentHashMap<String, String>()

    fun setIcon(url: String, icon: String) {
        val host = U.siteOf(url)
        if (host.isEmpty() || iconCache[host] == icon) return
        iconCache[host] = icon
        U.bg { db.execSQL("INSERT OR REPLACE INTO icons(host, icon) VALUES(?, ?)", arrayOf(host, icon)) }
    }

    fun iconFor(url: String): String? {
        val host = U.siteOf(url)
        if (host.isEmpty()) return null
        iconCache[host]?.let { return it }
        val c = db.rawQuery("SELECT icon FROM icons WHERE host=?", arrayOf(host))
        val r = if (c.moveToFirst()) c.getString(0) else null
        c.close()
        if (r != null) iconCache[host] = r
        return r
    }

    fun listJson(q: String, offset: Int, limit: Int): String {
        val items = JSONArray()
        val like = "%" + q.trim().replace("%", "") + "%"
        val c = db.rawQuery(
            "SELECT v.t, v.url, v.title, i.icon FROM visits v LEFT JOIN icons i ON i.host=v.host " +
                "WHERE v.url LIKE ? OR v.title LIKE ? ORDER BY v.t DESC LIMIT ? OFFSET ?",
            arrayOf(like, like, (limit + 1).toString(), offset.toString()),
        )
        var n = 0
        while (c.moveToNext()) {
            if (n++ >= limit) break
            items.put(U.obj("t" to c.getLong(0), "url" to c.getString(1), "title" to (c.getString(2) ?: ""), "icon" to (c.getString(3) ?: "")))
        }
        val more = c.count > limit
        c.close()
        return U.obj("type" to "hist", "items" to items, "offset" to offset, "more" to more).toString()
    }

    fun delete(items: JSONArray?) {
        items ?: return
        for (i in 0 until items.length()) {
            val o = items.optJSONObject(i) ?: continue
            db.delete("visits", "t=? AND url=?", arrayOf(o.optLong("t").toString(), o.optString("url")))
        }
    }

    fun clear(since: Long) {
        db.delete("visits", "t>=?", arrayOf(since.toString()))
    }

    /** The start page: the 8 most-visited sites of the last 90 days. */
    fun mostUsed(n: Int): JSONArray {
        val out = JSONArray()
        try {
            val c = db.rawQuery(
                "SELECT v.host, COUNT(*) AS k, (SELECT url FROM visits w WHERE w.host=v.host ORDER BY t DESC LIMIT 1), " +
                    "(SELECT title FROM visits w WHERE w.host=v.host AND title IS NOT NULL ORDER BY t DESC LIMIT 1), i.icon " +
                    "FROM visits v LEFT JOIN icons i ON i.host=v.host WHERE v.t>? GROUP BY v.host ORDER BY k DESC LIMIT ?",
                arrayOf((System.currentTimeMillis() - 90L * 86_400_000).toString(), (n + hidden.size).toString()),
            )
            while (c.moveToNext() && out.length() < n) {
                val host = c.getString(0)
                if (host in hidden) continue
                val name = host.substringBefore('.')
                val title = host.substringBeforeLast('.').replaceFirstChar { it.uppercase() }
                out.put(U.obj("url" to "https://$host/", "title" to title, "host" to host, "name" to name, "icon" to (c.getString(4) ?: "")))
            }
            c.close()
        } catch (e: Exception) {
            U.log("most used: $e")
        }
        return out
    }

    fun hideSite(host: String) {
        if (host.isEmpty()) return
        hidden.add(host)
        U.writeAtomic(File(U.dataDir(), "newtab_hidden.txt"), hidden.joinToString("\n"))
    }

    fun dbFile(): File = U.ctx.getDatabasePath("history.db")

    fun closeDb() = try { db.close() } catch (_: Exception) { }
}
