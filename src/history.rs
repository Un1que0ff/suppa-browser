// Browsing history: pages opened in normal (not incognito) tabs, kept in
// history.tsv in the data folder (time, address, title per line). The first
// time it runs, the engine's own history is copied in, so nothing from before
// is lost. Used by the History page (suppa.history) and the most-used
// shortcuts on the start page.
use std::cell::RefCell;
use std::collections::HashMap;

use crate::util;

const FILE: &str = "history.tsv";
const HIDDEN: &str = "shortcuts-hidden.txt";
const MAX: usize = 50_000;

pub struct Visit {
    pub t: i64, // Unix time, milliseconds
    pub url: String,
    pub title: String,
}

struct Hist {
    v: Vec<Visit>,
    dirty: usize,
}

thread_local! {
    static H: RefCell<Option<Hist>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Hist) -> R) -> R {
    H.with(|h| {
        let mut h = h.borrow_mut();
        let h = h.get_or_insert_with(load);
        f(h)
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

fn load() -> Hist {
    let path = util::data_dir().join(FILE);
    let v = match std::fs::read_to_string(&path) {
        Ok(text) => text
            .lines()
            .filter_map(|l| {
                let mut p = l.splitn(3, '\t');
                Some(Visit { t: p.next()?.parse().ok()?, url: p.next()?.to_string(), title: p.next().unwrap_or("").to_string() })
            })
            .collect(),
        Err(_) => {
            let v = import_engine();
            util::log(&format!("history: {} visits copied from the engine", v.len()));
            write(&v);
            v
        }
    };
    Hist { v, dirty: 0 }
}

fn write(v: &[Visit]) {
    let mut s = String::with_capacity(v.len() * 90);
    for x in v {
        s += &format!("{}\t{}\t{}\n", x.t, x.url, x.title);
    }
    let path = util::data_dir().join(FILE);
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, s).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Saves if anything changed (also called when Suppa closes).
pub fn save() {
    H.with(|h| {
        if let Some(h) = h.borrow_mut().as_mut() {
            if h.dirty > 0 {
                write(&h.v);
                h.dirty = 0;
            }
        }
    });
}

fn mark(h: &mut Hist) {
    h.dirty += 1;
    if h.dirty >= 25 {
        write(&h.v);
        h.dirty = 0;
    }
}

/// Only real web pages are kept (not Suppa's own pages, files or about:).
fn keep(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://")) && !util::host_of(url).starts_with("suppa.")
}

/// A page was opened in a normal tab.
pub fn record(url: &str, title: &str) {
    if !keep(url) {
        return;
    }
    let t = now_ms();
    with(|h| {
        // The same address again right away (reload, redirect back): one visit.
        if let Some(last) = h.v.last_mut().filter(|x| x.url == url && t - x.t < 60_000) {
            if !title.is_empty() {
                last.title = clean(title);
            }
            return;
        }
        h.v.push(Visit { t, url: url.to_string(), title: clean(title) });
        if h.v.len() > MAX {
            let extra = h.v.len() - MAX;
            h.v.drain(..extra);
        }
        mark(h);
    });
}

/// The page's title arrived (after the visit was recorded).
pub fn set_title(url: &str, title: &str) {
    if !keep(url) || title.is_empty() {
        return;
    }
    let t = now_ms();
    with(|h| {
        if let Some(x) = h.v.iter_mut().rev().take(20).find(|x| x.url == url && t - x.t < 600_000) {
            if x.title != title {
                x.title = clean(title);
                mark(h);
            }
        }
    });
}

/// For the History page: newest first, matching `q`, `limit` from `offset`.
pub fn list_json(q: &str, offset: usize, limit: usize) -> serde_json::Value {
    let q = q.trim().to_lowercase();
    with(|h| {
        let mut items = Vec::new();
        let mut matched = 0;
        for x in h.v.iter().rev() {
            if !q.is_empty() && !x.url.to_lowercase().contains(&q) && !x.title.to_lowercase().contains(&q) {
                continue;
            }
            matched += 1;
            if matched <= offset {
                continue;
            }
            if items.len() >= limit {
                break;
            }
            let icon = with_icons(|m| m.get(&site_of(&x.url)).cloned()).unwrap_or_default();
            items.push(serde_json::json!({ "t": x.t, "url": x.url, "title": x.title, "icon": icon }));
        }
        serde_json::json!({ "type": "hist", "items": items, "offset": offset, "more": items.len() >= limit, "total": h.v.len() })
    })
}

/// Deletes the given visits (time + address).
pub fn delete(list: &[(i64, String)]) {
    with(|h| {
        h.v.retain(|x| !list.iter().any(|(t, u)| *t == x.t && *u == x.url));
        mark(h);
        write(&h.v);
        h.dirty = 0;
    });
}

/// Deletes every visit since `since_ms` (0 = all of it).
pub fn clear(since_ms: i64) {
    with(|h| {
        h.v.retain(|x| x.t < since_ms);
        write(&h.v);
        h.dirty = 0;
    });
    if since_ms <= 0 {
        clear_icons();
    }
}

// ---- site icons (start page shortcuts) -------------------------------------------------
// The icon address of each site ("site" = host without "www."), learned when a
// page in a normal tab gets its icon. Kept in site-icons.tsv.

const ICONS: &str = "site-icons.tsv";

thread_local! {
    static ICON_MAP: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
}

fn with_icons<R>(f: impl FnOnce(&mut HashMap<String, String>) -> R) -> R {
    ICON_MAP.with(|m| {
        let mut m = m.borrow_mut();
        let m = m.get_or_insert_with(|| match std::fs::read_to_string(util::data_dir().join(ICONS)) {
            Ok(t) => t.lines().filter_map(|l| l.split_once('\t')).map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            Err(_) => {
                // First time: the icons the engine already knows.
                let m = engine_icons();
                util::log(&format!("history: {} site icons copied from the engine", m.len()));
                write_icons(&m);
                m
            }
        });
        f(m)
    })
}

fn write_icons(m: &HashMap<String, String>) {
    let text: String = m.iter().map(|(a, b)| format!("{a}\t{b}\n")).collect();
    let _ = std::fs::write(util::data_dir().join(ICONS), text);
}

/// The icon of a site, if known ("site" = host without "www.").
pub fn icon_for(url: &str) -> Option<String> {
    let site = site_of(url);
    with_icons(|m| m.get(&site).cloned())
}

/// Site -> icon address from the engine's own Favicons database.
fn engine_icons() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let db = util::sub_dir("UserData").join("EBWebView").join("Default").join("Favicons");
    if !db.exists() {
        return out;
    }
    let tmp = util::data_dir().join("favicons-read.tmp");
    if std::fs::copy(&db, &tmp).is_err() {
        return out;
    }
    let r = (|| -> Result<(), String> {
        let conn = crate::sqlite::Sqlite::load()?.open(&tmp)?;
        let mut st = conn.prepare("SELECT m.page_url, f.url FROM icon_mapping m JOIN favicons f ON f.id = m.icon_id")?;
        while st.next()? {
            let (page, icon) = (st.col_text(0), st.col_text(1));
            if keep(&page) && icon.starts_with("http") {
                let site = site_of(&page);
                // A site's front page icon wins over its other pages' icons.
                let front = page.splitn(4, '/').nth(3).unwrap_or("").is_empty();
                if !site.is_empty() && (front || !out.contains_key(&site)) {
                    out.insert(site, icon);
                }
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    if let Err(e) = r {
        util::log(&format!("history: engine icons not read: {e}"));
    }
    out
}

fn site_of(url: &str) -> String {
    let rest = url.split_once("://").map(|x| x.1).unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

/// A page in a normal tab got its icon.
pub fn learn_icon(page_url: &str, icon: &str) {
    if !keep(page_url) || !icon.starts_with("http") {
        return;
    }
    let site = site_of(page_url);
    if site.is_empty() {
        return;
    }
    with_icons(|m| {
        if m.get(&site).map(|x| x.as_str()) != Some(icon) {
            m.insert(site, icon.to_string());
            write_icons(m);
        }
    });
}

fn clear_icons() {
    // Kept as an empty file, so the engine's icons are not copied in again.
    with_icons(|m| {
        m.clear();
        write_icons(m);
    });
}

// ---- most-used sites (start page shortcuts) ------------------------------------------

fn hidden() -> Vec<String> {
    std::fs::read_to_string(util::data_dir().join(HIDDEN)).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
}

/// Removes a site from the shortcuts (it comes back only if cleared in Settings).
pub fn hide_site(host: &str) {
    let mut h = hidden();
    if !h.iter().any(|x| x == host) {
        h.push(host.to_string());
        let _ = std::fs::write(util::data_dir().join(HIDDEN), h.join("\n"));
    }
}

/// The sites visited most in the last 90 days: [{url, title, host}].
pub fn most_used(n: usize) -> serde_json::Value {
    let since = now_ms() - 90 * 24 * 3600 * 1000;
    let hide = hidden();
    with(|h| {
        // site -> (visits, front page url, front page title, any title, last visit)
        let mut m: HashMap<String, (usize, String, String, String, i64)> = HashMap::new();
        for x in h.v.iter().filter(|x| x.t >= since) {
            // Site = scheme + host + port ("http://localhost:8765" stays one site);
            // "www.site.com" and "site.com" are the same site.
            let (scheme, rest) = x.url.split_once("://").unwrap_or(("https", ""));
            let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
            let site = host.strip_prefix("www.").unwrap_or(&host).to_string();
            if site.is_empty() || hide.contains(&site) || hide.contains(&host) {
                continue;
            }
            let e = m.entry(site).or_insert((0, format!("{scheme}://{host}/"), String::new(), String::new(), 0));
            e.0 += 1;
            let path = x.url.splitn(4, '/').nth(3).unwrap_or("");
            if !x.title.is_empty() {
                if path.is_empty() {
                    e.2 = x.title.clone();
                }
                e.3 = x.title.clone();
            }
            e.4 = e.4.max(x.t);
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(b.1.4.cmp(&a.1.4)));
        let items: Vec<_> = v
            .into_iter()
            .take(n)
            .map(|(site, (_, url, front, any, _))| {
                let title = short_title(if front.is_empty() { &any } else { &front }, &site, !front.is_empty());
                let icon = with_icons(|m| m.get(&site).cloned()).unwrap_or_default();
                serde_json::json!({ "url": url, "title": title, "host": site, "name": main_label(&site), "icon": icon })
            })
            .collect();
        serde_json::Value::Array(items)
    })
}

/// "en.wikipedia.org" -> "wikipedia", "localhost:8765" -> "localhost"
fn main_label(site: &str) -> String {
    let host = site.split(':').next().unwrap_or(site);
    let labels: Vec<&str> = host.split('.').collect();
    let l = if labels.len() >= 2 { labels[labels.len() - 2] } else { host };
    l.to_lowercase()
}

/// A short name for a site from a page title: the part naming the site
/// ("Rust (programming language) - Wikipedia" -> "Wikipedia",
/// "GitHub · Change is constant" -> "GitHub"); else the front page's own title
/// ("Hacker News"); else the site's name ("Odoo") or address.
fn short_title(title: &str, site: &str, front_page: bool) -> String {
    let label = main_label(site).replace('-', "");
    let mut t = title.to_string();
    for sep in [" - ", " | ", " · ", " \u{2014} ", " \u{2013} "] {
        t = t.replace(sep, "\u{1}");
    }
    let parts: Vec<&str> = t.split('\u{1}').map(str::trim).filter(|p| !p.is_empty()).collect();
    let named = parts.iter().find(|p| p.to_lowercase().replace([' ', '-'], "").contains(&label)).copied().unwrap_or("");
    if !named.is_empty() && named.chars().count() <= 24 {
        return named.to_string();
    }
    // The front page's title names the site even without its address in it
    // ("Hacker News" on news.ycombinator.com, "Rust Documentation").
    if front_page {
        if let Some(first) = parts.first().filter(|p| p.chars().count() <= 24) {
            return first.to_string();
        }
    }
    // Only a deeper page's title ("WH/MO/00189", "Before you continue"): the
    // site's name ("Odoo", "Google"); for a subdomain ("doc.rust-lang.org"),
    // localhost or an IP address, its address.
    let word = main_label(site);
    let host = site.split(':').next().unwrap_or(site);
    let sub = host.split('.').count() > 2;
    if !sub && word.len() > 1 && word.chars().all(|c| c.is_ascii_alphabetic() || c == '-') && word.starts_with(|c: char| c.is_ascii_alphabetic()) && word != "localhost" {
        let mut c = word.chars();
        c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
    } else {
        site.to_string()
    }
}
// ---- first run: copy the engine's own history ------------------------------------------

fn import_engine() -> Vec<Visit> {
    let db = util::sub_dir("UserData").join("EBWebView").join("Default").join("History");
    if !db.exists() {
        return Vec::new();
    }
    let tmp = util::data_dir().join("history-read.tmp");
    if std::fs::copy(&db, &tmp).is_err() {
        return Vec::new();
    }
    let result = (|| -> Result<Vec<Visit>, String> {
        let conn = crate::sqlite::Sqlite::load()?.open(&tmp)?;
        let mut st = conn.prepare(&format!(
            "SELECT v.visit_time, u.url, u.title FROM visits v JOIN urls u ON u.id = v.url ORDER BY v.visit_time DESC LIMIT {MAX}"
        ))?;
        let mut out = Vec::new();
        while st.next()? {
            // Windows microseconds since 1601 -> Unix milliseconds
            let t = st.col_int(0) / 1000 - 11_644_473_600_000;
            let url = st.col_text(1);
            if t > 0 && keep(&url) {
                out.push(Visit { t, url, title: clean(&st.col_text(2)) });
            }
        }
        out.reverse();
        Ok(out)
    })();
    let _ = std::fs::remove_file(&tmp);
    result.unwrap_or_else(|e| {
        util::log(&format!("history: engine history not read: {e}"));
        Vec::new()
    })
}
