package com.suppa.browser

import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * The ad and tracker blocker (adblock.rs): the same filter lists as on Windows
 * (EasyList, EasyPrivacy, uBlock filters and quick fixes, the cookie-banner
 * list) plus your own rules (.txt files in scripts/filters), in EasyList / uBlock
 * syntax. Network rules block requests; element-hiding rules hide ads with CSS.
 * (Scriptlets and procedural cosmetic rules of uBlock are not supported here.)
 */
object AdBlock {
    private val LISTS = listOf(
        "easylist.txt" to "https://easylist.to/easylist/easylist.txt",
        "easyprivacy.txt" to "https://easylist.to/easylist/easyprivacy.txt",
        "ublock-filters.txt" to "https://ublockorigin.github.io/uAssets/filters/filters.txt",
        "ublock-quick-fixes.txt" to "https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt",
        "cookie-banners.txt" to "https://secure.fanboy.co.nz/fanboy-cookiemonster.txt",
    )
    private const val MAX_AGE = 4L * 86_400_000

    private class Rule(
        val pattern: String, val hostAnchor: Boolean, val start: Boolean, val end: Boolean,
        val party: Int, val domains: Set<String>?, val notDomains: Set<String>?, val types: Set<String>?, val notTypes: Set<String>?,
    )

    private class Engine {
        val blockHosts = HashSet<String>()
        val allowHosts = HashSet<String>()
        val docAllow = HashSet<String>()
        val block = HashMap<String, MutableList<Rule>>()
        val allow = HashMap<String, MutableList<Rule>>()
        val blockAny = mutableListOf<Rule>()
        val allowAny = mutableListOf<Rule>()
        val generic = LinkedHashSet<String>()
        val specific = HashMap<String, MutableList<String>>()
        val specificAllow = HashMap<String, MutableSet<String>>()
        var rules = 0
        var genericCss = ""
    }

    @Volatile private var engine: Engine? = null
    @Volatile var ready = false
        private set

    private fun dir() = File(U.dataDir(), "filters").also { it.mkdirs() }

    /** Start: build from the downloaded lists (downloading them first when missing or old). */
    fun init() {
        U.bg {
            val missing = LISTS.any { !File(dir(), it.first).isFile }
            if (missing) download()
            build()
            val oldest = LISTS.minOf { File(dir(), it.first).lastModified() }
            if (System.currentTimeMillis() - oldest > MAX_AGE) {
                download()
                build()
            }
        }
    }

    fun update(force: Boolean) {
        if (force) Popup.message("Ad blocker", "Updating the filter lists...")
        U.bg {
            val n = download()
            build()
            if (force) U.ui { Popup.message("Ad blocker", "$n of ${LISTS.size} filter lists updated - ${engine?.rules ?: 0} rules.") }
        }
    }

    private fun download(): Int {
        var ok = 0
        for ((name, url) in LISTS) {
            try {
                val c = URL(url).openConnection() as HttpURLConnection
                c.connectTimeout = 15000
                c.readTimeout = 30000
                c.setRequestProperty("User-Agent", "Suppa/${U.version()}")
                if (c.responseCode == 200) {
                    val text = c.inputStream.bufferedReader().use { it.readText() }
                    if (text.length > 1000) {
                        U.writeAtomic(File(dir(), name), text)
                        ok++
                    }
                }
                c.disconnect()
            } catch (e: Exception) {
                U.log("filter list $name: $e")
            }
        }
        U.log("ad blocker: $ok filter lists downloaded")
        return ok
    }

    private fun build() {
        val t0 = System.currentTimeMillis()
        val e = Engine()
        for ((name, _) in LISTS) File(dir(), name).takeIf { it.isFile }?.forEachLine { parse(e, it) }
        // Your own rules: the built-in example file, or the files in your scripts folder.
        val own = File(U.scriptsDir(), "filters").listFiles { f -> f.name.endsWith(".txt") }
        if (own.isNullOrEmpty()) U.script("filters/my-filters.txt")?.lines()?.forEach { parse(e, it) }
        else own.forEach { f -> f.forEachLine { parse(e, it) } }
        e.genericCss = css(e.generic.toList())
        engine = e
        ready = true
        U.log("ad blocker ready: ${e.rules} rules in ${System.currentTimeMillis() - t0} ms")
        U.ui { Content.invalidate() }
    }

    // ---- parsing -------------------------------------------------------------------------

    private val PROCEDURAL = listOf(
        ":has-text(", ":-abp-", ":xpath(", ":style(", ":remove(", ":matches-css", ":upward(", ":min-text-length(",
        ":watch-attr(", ":matches-path(", ":others(", ":if(", ":if-not(", ":nth-ancestor(", ":remove-attr(",
        ":remove-class(", ":matches-attr(", ":matches-prop(", ":spath(", ":shadow", ":contains(", ":matches-media(",
    )
    private val SKIP_OPTS = setOf(
        "document", "doc", "popup", "csp", "redirect", "redirect-rule", "removeparam", "badfilter", "header", "permissions",
        "replace", "urltransform", "uritransform", "cname", "inline-script", "inline-font", "denyallow", "to", "popunder",
        "generichide", "ghide", "elemhide", "ehide", "specifichide", "shide", "urlskip", "rewrite", "empty", "mp4",
    )
    private val TYPE_ALIAS = mapOf("css" to "stylesheet", "xhr" to "xmlhttprequest", "frame" to "subdocument", "iframe" to "subdocument")

    private fun parse(e: Engine, raw: String) {
        val line = raw.trim()
        if (line.isEmpty() || line.startsWith("!") || line.startsWith("[")) return
        // Element hiding: domains##selector, domains#@#selector
        val hide = line.indexOf("##")
        val unhide = line.indexOf("#@#")
        if (hide >= 0 || unhide >= 0) {
            val allow = unhide >= 0 && (hide < 0 || unhide < hide)
            val at = if (allow) unhide else hide
            val sel = line.substring(at + if (allow) 3 else 2).trim()
            if (sel.isEmpty() || sel.startsWith("+js") || sel.startsWith("^") || sel.startsWith("+") || PROCEDURAL.any { sel.contains(it) }) return
            val doms = line.substring(0, at).split(',').map { it.trim().lowercase() }.filter { it.isNotEmpty() }
            val pos = doms.filter { !it.startsWith("~") && !it.endsWith(".*") }
            if (allow) {
                if (pos.isEmpty()) e.generic.remove(sel) else pos.forEach { e.specificAllow.getOrPut(it) { HashSet() }.add(sel) }
            } else if (doms.none { !it.startsWith("~") }) {
                e.generic.add(sel)
            } else {
                pos.forEach { e.specific.getOrPut(it) { mutableListOf() }.add(sel) }
            }
            e.rules++
            return
        }
        if (line.contains("#$#") || line.contains("#?#") || line.contains("#%#")) return
        var text = line
        val exception = text.startsWith("@@")
        if (exception) text = text.substring(2)
        // Regular-expression rules are skipped (too slow to run on every request on a phone).
        if (text.startsWith("/") && text.length > 2 && (text.endsWith("/") || text.contains("/$"))) return
        var opts: List<String> = emptyList()
        val dollar = text.lastIndexOf('$')
        if (dollar >= 0 && !text.substring(dollar).contains('/')) {
            opts = text.substring(dollar + 1).split(',').map { it.trim().lowercase() }.filter { it.isNotEmpty() }
            text = text.substring(0, dollar)
        }
        var party = -1
        var domains: MutableSet<String>? = null
        var notDomains: MutableSet<String>? = null
        var types: MutableSet<String>? = null
        var notTypes: MutableSet<String>? = null
        var isDoc = false
        for (o in opts) {
            val name = o.substringBefore('=')
            when {
                name == "third-party" || name == "3p" -> party = 1
                name == "~third-party" || name == "1p" || name == "first-party" || name == "~3p" -> party = 0
                name == "domain" || name == "from" -> for (d in o.substringAfter('=').split('|')) {
                    if (d.startsWith("~")) (notDomains ?: HashSet<String>().also { notDomains = it }).add(d.drop(1))
                    else if (!d.endsWith(".*")) (domains ?: HashSet<String>().also { domains = it }).add(d)
                }
                name == "important" || name == "match-case" || name == "all" -> {}
                (name == "document" || name == "doc") && exception -> isDoc = true
                name in SKIP_OPTS -> return
                name.startsWith("~") -> (notTypes ?: HashSet<String>().also { notTypes = it }).add(TYPE_ALIAS[name.drop(1)] ?: name.drop(1))
                else -> (types ?: HashSet<String>().also { types = it }).add(TYPE_ALIAS[name] ?: name)
            }
        }
        val p = text.lowercase()
        if (p.isEmpty() || p == "*" || p == "|" || p == "||") return
        // Plain "||example.com^": a host list (fast).
        val plainHost = p.startsWith("||") && p.endsWith("^") && p.length > 3 && p.substring(2, p.length - 1).all { it.isLetterOrDigit() || it == '.' || it == '-' }
        if (isDoc) {
            if (plainHost) e.docAllow.add(p.substring(2, p.length - 1))
            return
        }
        if (plainHost && party < 0 && domains == null && notDomains == null && types == null && notTypes == null) {
            (if (exception) e.allowHosts else e.blockHosts).add(p.substring(2, p.length - 1))
            e.rules++
            return
        }
        val hostAnchor = p.startsWith("||")
        val start = !hostAnchor && p.startsWith("|")
        val end = p.endsWith("|") && !p.endsWith("||")
        var body = p
        if (hostAnchor) body = body.substring(2) else if (start) body = body.substring(1)
        if (end) body = body.dropLast(1)
        val rule = Rule(body, hostAnchor, start, end, party, domains, notDomains, types, notTypes)
        val token = bestToken(body, hostAnchor || start, end)
        if (token == null) (if (exception) e.allowAny else e.blockAny).add(rule)
        else (if (exception) e.allow else e.block).getOrPut(token) { mutableListOf() }.add(rule)
        e.rules++
    }

    /** The longest run of letters/digits that every matching address must contain. */
    private fun bestToken(p: String, startAnchored: Boolean, endAnchored: Boolean): String? {
        var best: String? = null
        var i = 0
        while (i < p.length) {
            if (!p[i].isLetterOrDigit()) { i++; continue }
            val s = i
            while (i < p.length && p[i].isLetterOrDigit()) i++
            // Only a whole word of the address: not next to a wildcard or an open end.
            val okStart = if (s == 0) startAnchored else p[s - 1] != '*'
            val okEnd = if (i == p.length) endAnchored else p[i] != '*'
            if (!okStart || !okEnd) continue
            val tok = p.substring(s, i)
            if (tok.length >= 3 && (best == null || tok.length > best.length) && tok != "http" && tok != "https" && tok != "www" && tok != "com") best = tok
        }
        return best
    }

    // ---- matching ------------------------------------------------------------------------

    private fun isSep(c: Char) = !(c.isLetterOrDigit() || c == '_' || c == '-' || c == '.' || c == '%')

    /** Does pattern p (from pi) match s (from si), with * and ^? */
    private fun match(p: String, pi0: Int, s: String, si0: Int, end: Boolean): Boolean {
        var pi = pi0
        var si = si0
        while (pi < p.length) {
            val c = p[pi]
            when (c) {
                '*' -> {
                    while (pi < p.length && p[pi] == '*') pi++
                    if (pi == p.length) return true
                    for (k in si..s.length) if (match(p, pi, s, k, end)) return true
                    return false
                }
                '^' -> {
                    if (si == s.length) { pi++; continue }
                    if (!isSep(s[si])) return false
                    pi++; si++
                }
                else -> {
                    if (si >= s.length || s[si] != c) return false
                    pi++; si++
                }
            }
        }
        return !end || si == s.length
    }

    private fun base(h: String): String {
        val p = h.split('.')
        if (p.size <= 2) return h
        return if (p[p.size - 2].length <= 3 && p.last().length == 2) p.takeLast(3).joinToString(".") else p.takeLast(2).joinToString(".")
    }

    private fun hostIn(host: String, set: Set<String>): Boolean {
        if (set.isEmpty()) return false
        var h = host
        while (true) {
            if (h in set) return true
            val i = h.indexOf('.')
            if (i < 0) return false
            h = h.substring(i + 1)
        }
    }

    private fun guessType(url: String, accept: String): String {
        val path = url.substringBefore('?').substringBefore('#')
        val ext = path.substringAfterLast('/').substringAfterLast('.', "")
        return when {
            accept.startsWith("image/") || ext in setOf("png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "avif", "bmp") -> "image"
            accept.startsWith("text/css") || ext == "css" -> "stylesheet"
            ext == "js" || ext == "mjs" -> "script"
            ext in setOf("woff", "woff2", "ttf", "otf", "eot") -> "font"
            ext in setOf("mp4", "webm", "mp3", "m3u8", "ts", "ogg", "m4a") -> "media"
            accept.startsWith("text/html") -> "subdocument"
            else -> "xmlhttprequest"
        }
    }

    private fun ruleMatches(r: Rule, url: String, host: String, pageHost: String, third: Boolean, type: () -> String): Boolean {
        if (r.party == 1 && !third) return false
        if (r.party == 0 && third) return false
        if (r.domains != null && !hostIn(pageHost, r.domains)) return false
        if (r.notDomains != null && hostIn(pageHost, r.notDomains)) return false
        if (r.types != null && type() !in r.types) return false
        if (r.notTypes != null && type() in r.notTypes) return false
        val p = r.pattern
        if (r.hostAnchor) {
            val hs = url.indexOf("://")
            if (hs < 0) return false
            var i = hs + 3
            val hostEnd = i + host.length
            while (i <= hostEnd) {
                if ((i == hs + 3 || url[i - 1] == '.') && match(p, 0, url, i, r.end)) return true
                val dot = url.indexOf('.', i)
                if (dot < 0 || dot >= hostEnd) break
                i = dot + 1
            }
            return false
        }
        if (r.start) return match(p, 0, url, 0, r.end)
        // Unanchored: wherever the first literal part appears.
        val lit = p.takeWhile { it != '*' && it != '^' }
        if (lit.isEmpty()) {
            for (i in 0..url.length) if (match(p, 0, url, i, r.end)) return true
            return false
        }
        var i = url.indexOf(lit)
        while (i >= 0) {
            if (match(p, 0, url, i, r.end)) return true
            i = url.indexOf(lit, i + 1)
        }
        return false
    }

    private fun anyRule(index: Map<String, List<Rule>>, any: List<Rule>, tokens: List<String>, url: String, host: String, page: String, third: Boolean, type: () -> String): Boolean {
        for (t in tokens) index[t]?.let { list -> for (r in list) if (ruleMatches(r, url, host, page, third, type)) return true }
        for (r in any) if (ruleMatches(r, url, host, page, third, type)) return true
        return false
    }

    private fun tokens(url: String): List<String> {
        val out = ArrayList<String>(24)
        var i = 0
        while (i < url.length) {
            if (!url[i].isLetterOrDigit()) { i++; continue }
            val s = i
            while (i < url.length && url[i].isLetterOrDigit()) i++
            if (i - s >= 3) out.add(url.substring(s, i))
        }
        return out
    }

    /** A request of a page (not the page itself): blocked? Called on the network thread. */
    fun shouldBlock(rawUrl: String, pageUrl: String, accept: String): Boolean {
        val e = engine ?: return false
        if (!rawUrl.startsWith("http")) return false
        val url = rawUrl.lowercase()
        val host = U.hostOf(url)
        val pageHost = U.hostOf(pageUrl)
        if (host.isEmpty() || hostIn(pageHost, e.docAllow)) return false
        val third = base(host) != base(pageHost)
        var typeCache: String? = null
        val type = { typeCache ?: guessType(url, accept).also { typeCache = it } }
        val toks = tokens(url)
        val blocked = hostIn(host, e.blockHosts) || anyRule(e.block, e.blockAny, toks, url, host, pageHost, third, type)
        if (!blocked) return false
        if (hostIn(host, e.allowHosts)) return false
        return !anyRule(e.allow, e.allowAny, toks, url, host, pageHost, third, type)
    }

    // ---- element hiding ------------------------------------------------------------------

    private fun css(sels: List<String>): String {
        if (sels.isEmpty()) return ""
        // :is() skips a selector it doesn't understand instead of dropping the whole rule.
        return sels.chunked(400).joinToString("\n") { ":is(" + it.joinToString(",") + "){display:none!important}" }
    }

    /** Hiding rules for every site (added by the page script at document start). */
    fun genericCss(): String = engine?.genericCss ?: ""

    /** Hiding rules for one site (and its parent domains). */
    fun cosmeticCss(host: String): String {
        val e = engine ?: return ""
        val out = LinkedHashSet<String>()
        val except = HashSet<String>()
        var h = host.lowercase()
        while (true) {
            e.specific[h]?.let { out.addAll(it) }
            e.specificAllow[h]?.let { except.addAll(it) }
            val i = h.indexOf('.')
            if (i < 0) break
            h = h.substring(i + 1)
        }
        out.removeAll(except)
        var css = css(out.toList())
        // Generic rules turned off on this site.
        if (except.any { it in e.generic }) css += "\n" + except.filter { it in e.generic }.chunked(400).joinToString("\n") { ":is(" + it.joinToString(",") + "){display:revert!important}" }
        return css
    }
}
