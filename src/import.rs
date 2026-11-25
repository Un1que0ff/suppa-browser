// Import bookmarks from other browsers into the saved-tabs bar.
//  * Chrome, Edge, Brave, Vivaldi, Opera: their "Bookmarks" file (JSON).
//  * Firefox: its automatic bookmark backups (bookmarkbackups\*.jsonlz4).
//  * Any browser: an exported bookmarks .html file.
// Their bookmarks bar becomes Suppa's saved-tabs bar (folders stay folders;
// folders inside folders are flattened, the saved bar has one folder level).
// Everything else goes into one folder "<Browser> - other bookmarks".
// The other browsers' files are only read, never changed.
//
// Passwords come from the other browser's own export file (.csv), see passwords.rs.
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::popup::Item;
use crate::saved::{Link, Node};
use crate::util;

struct Source {
    name: &'static str,
    file: PathBuf,
    firefox: bool,
}

fn env_dir(var: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(var).unwrap_or_default())
}

/// Browsers found on this PC.
fn sources() -> Vec<Source> {
    let local = env_dir("LOCALAPPDATA");
    let roaming = env_dir("APPDATA");
    let mut out = Vec::new();
    let chromium = [
        ("Chrome", local.join(r"Google\Chrome\User Data\Default\Bookmarks")),
        ("Edge", local.join(r"Microsoft\Edge\User Data\Default\Bookmarks")),
        ("Brave", local.join(r"BraveSoftware\Brave-Browser\User Data\Default\Bookmarks")),
        ("Vivaldi", local.join(r"Vivaldi\User Data\Default\Bookmarks")),
        ("Opera", roaming.join(r"Opera Software\Opera Stable\Bookmarks")),
        ("Opera GX", roaming.join(r"Opera Software\Opera GX Stable\Bookmarks")),
    ];
    for (name, file) in chromium {
        if file.exists() {
            out.push(Source { name, file, firefox: false });
        }
    }
    if let Some(file) = newest_firefox_backup(&roaming.join(r"Mozilla\Firefox\Profiles")) {
        out.push(Source { name: "Firefox", file, firefox: true });
    }
    out
}

fn newest_firefox_backup(profiles: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for p in std::fs::read_dir(profiles).ok()?.flatten() {
        let Ok(rd) = std::fs::read_dir(p.path().join("bookmarkbackups")) else { continue };
        for f in rd.flatten() {
            let path = f.path();
            if !path.to_string_lossy().ends_with(".jsonlz4") {
                continue;
            }
            let t = f.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
                best = Some((t, path));
            }
        }
    }
    best.map(|(_, p)| p)
}

// ---- Chromium (Chrome, Edge, Brave, ...) ------------------------------------------------

fn chromium_links(node: &Value, out: &mut Vec<Link>) {
    match node.get("type").and_then(|t| t.as_str()) {
        Some("url") => {
            if let Some(url) = node.get("url").and_then(|u| u.as_str()) {
                let title = node.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                out.push(Link { title, url: url.to_string(), icon: String::new() });
            }
        }
        _ => {
            for c in node.get("children").and_then(|c| c.as_array()).into_iter().flatten() {
                chromium_links(c, out);
            }
        }
    }
}

fn from_chromium(json: &Value, browser: &str) -> Vec<Node> {
    let mut out = Vec::new();
    let roots = &json["roots"];
    for item in roots["bookmark_bar"]["children"].as_array().into_iter().flatten() {
        if item["type"] == "url" {
            let mut l = Vec::new();
            chromium_links(item, &mut l);
            out.extend(l.into_iter().map(Node::Link));
        } else {
            let mut links = Vec::new();
            chromium_links(item, &mut links);
            let title = item["name"].as_str().unwrap_or("Folder").to_string();
            out.push(Node::Folder { title, items: links });
        }
    }
    let mut other = Vec::new();
    chromium_links(&roots["other"], &mut other);
    chromium_links(&roots["synced"], &mut other);
    if !other.is_empty() {
        out.push(Node::Folder { title: format!("{browser} - other bookmarks"), items: other });
    }
    out
}

// ---- Firefox (bookmarkbackups\*.jsonlz4) -------------------------------------------------

/// Decodes an LZ4 block (Firefox's "mozlz4" files).
fn lz4_block(src: &[u8], size_hint: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size_hint);
    let mut i = 0;
    let read_len = |i: &mut usize, mut n: usize| -> Option<usize> {
        loop {
            let b = *src.get(*i)?;
            *i += 1;
            n += b as usize;
            if b != 255 {
                return Some(n);
            }
        }
    };
    while i < src.len() {
        let token = src[i];
        i += 1;
        let mut lit = (token >> 4) as usize;
        if lit == 15 {
            lit = read_len(&mut i, lit)?;
        }
        out.extend_from_slice(src.get(i..i + lit)?);
        i += lit;
        if i >= src.len() {
            break;
        }
        let off = u16::from_le_bytes([*src.get(i)?, *src.get(i + 1)?]) as usize;
        i += 2;
        if off == 0 || off > out.len() {
            return None;
        }
        let mut ml = (token & 15) as usize;
        if ml == 15 {
            ml = read_len(&mut i, ml)?;
        }
        ml += 4;
        let start = out.len() - off;
        for k in 0..ml {
            let b = out[start + k];
            out.push(b);
        }
    }
    Some(out)
}

fn read_mozlz4(path: &Path) -> Option<Value> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 12 || &b[0..8] != b"mozLz40\0" {
        return None;
    }
    let size = u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize;
    let raw = lz4_block(&b[12..], size)?;
    serde_json::from_slice(&raw).ok()
}

fn firefox_links(node: &Value, out: &mut Vec<Link>) {
    if let Some(uri) = node.get("uri").and_then(|u| u.as_str()) {
        if uri.starts_with("http") {
            let title = node.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
            out.push(Link { title, url: uri.to_string(), icon: String::new() });
        }
    }
    for c in node.get("children").and_then(|c| c.as_array()).into_iter().flatten() {
        firefox_links(c, out);
    }
}

fn from_firefox(json: &Value) -> Vec<Node> {
    let mut out = Vec::new();
    let mut other = Vec::new();
    for root in json["children"].as_array().into_iter().flatten() {
        if root["guid"] == "toolbar_____" {
            for item in root["children"].as_array().into_iter().flatten() {
                if item.get("uri").is_some() {
                    let mut l = Vec::new();
                    firefox_links(item, &mut l);
                    out.extend(l.into_iter().map(Node::Link));
                } else if item.get("children").is_some() {
                    let mut links = Vec::new();
                    firefox_links(item, &mut links);
                    let title = item["title"].as_str().unwrap_or("Folder").to_string();
                    out.push(Node::Folder { title, items: links });
                }
            }
        } else {
            firefox_links(root, &mut other);
        }
    }
    if !other.is_empty() {
        out.push(Node::Folder { title: "Firefox - other bookmarks".into(), items: other });
    }
    out
}

// ---- Bookmarks .html export (any browser) -------------------------------------------------

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

/// Netscape bookmark file: <DT><H3>Folder</H3><DL> ... <DT><A HREF="...">Title</A> ... </DL>
fn from_html(text: &str) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();
    // depth 1 = inside the first <DL>; the "Bookmarks bar" folder counts as top level.
    let mut depth = 0usize;
    let mut folder_depth: Option<usize> = None;
    let mut pending_folder: Option<String> = None;
    let lower_all = text.to_ascii_lowercase();
    let mut pos = 0usize;
    while let Some(lt) = lower_all[pos..].find('<') {
        let at = pos + lt;
        let tag_end = match lower_all[at..].find('>') {
            Some(e) => at + e,
            None => break,
        };
        let tag = &lower_all[at..=tag_end];
        if tag.starts_with("<dl") {
            depth += 1;
            if let Some(title) = pending_folder.take() {
                let bar = title.eq_ignore_ascii_case("bookmarks bar") || title.eq_ignore_ascii_case("bookmarks toolbar")
                    || title.eq_ignore_ascii_case("favorites bar");
                if !bar && folder_depth.is_none() {
                    out.push(Node::Folder { title, items: Vec::new() });
                    folder_depth = Some(depth);
                }
            }
        } else if tag.starts_with("</dl") {
            if folder_depth == Some(depth) {
                folder_depth = None;
            }
            depth = depth.saturating_sub(1);
        } else if tag.starts_with("<h3") {
            let close = lower_all[tag_end..].find("</h3>").map(|c| tag_end + c).unwrap_or(tag_end);
            pending_folder = Some(html_unescape(text[tag_end + 1..close].trim()));
        } else if tag.starts_with("<a ") {
            let href = tag.find("href=\"").map(|h| {
                let s = at + h + 6;
                let e = lower_all[s..].find('"').map(|e| s + e).unwrap_or(s);
                html_unescape(&text[s..e])
            });
            let close = lower_all[tag_end..].find("</a>").map(|c| tag_end + c).unwrap_or(tag_end);
            let title = html_unescape(text[tag_end + 1..close].trim());
            if let Some(url) = href.filter(|u| u.starts_with("http")) {
                let link = Link { title, url, icon: String::new() };
                match (folder_depth, out.last_mut()) {
                    (Some(_), Some(Node::Folder { items, .. })) => items.push(link),
                    _ => out.push(Node::Link(link)),
                }
            }
        }
        pos = tag_end + 1;
    }
    out
}

// ---- Menu + running an import ------------------------------------------------------------

/// Items for the ≡ -> "Backup and import" submenu (ids 70_000+).
pub fn menu_items() -> Vec<Item> {
    let mut items = vec![
        Item::new(BACKUP, "Back up settings, saved tabs and passwords..."),
        Item::new(RESTORE, "Restore from a backup..."),
        Item::sep(),
    ];
    let found = sources();
    for (i, s) in found.iter().enumerate() {
        items.push(Item::new(IMPORT_BASE + i as i64, format!("Import bookmarks from {}", s.name)));
    }
    if found.is_empty() {
        items.push(Item::new(0, "No other browsers found").enabled(false));
    }
    items.push(Item::new(IMPORT_FILE, "Import bookmarks from a file (.html)..."));
    items.push(Item::sep());
    items.push(Item::new(IMPORT_PASSWORDS, "Import passwords from a file (Firefox, Chrome, Edge .csv)..."));
    items
}

pub const BACKUP: i64 = 70_000;
pub const RESTORE: i64 = 70_001;
pub const IMPORT_FILE: i64 = 70_002;
pub const IMPORT_PASSWORDS: i64 = 70_003;
pub const IMPORT_BASE: i64 = 70_100;

/// Runs a choice from `menu_items`. Returns false if the id isn't ours.
pub fn run(id: i64) -> bool {
    match id {
        BACKUP => crate::backup::backup(),
        RESTORE => crate::backup::restore(),
        IMPORT_PASSWORDS => crate::passwords::import_file(),
        IMPORT_FILE => {
            if let Some(f) = util::open_dialog("Import bookmarks", "Bookmarks file", "*.html;*.htm") {
                let text = std::fs::read_to_string(&f).unwrap_or_default();
                finish("the file", from_html(&text));
            }
        }
        i if (IMPORT_BASE..IMPORT_BASE + 20).contains(&i) => import_browser((i - IMPORT_BASE) as usize),
        _ => return false,
    }
    true
}

/// Import from browser number `i` of `sources()`.
pub fn import_browser(i: usize) {
    let found = sources();
    let Some(s) = found.get(i) else { return };
    let nodes = if s.firefox {
        read_mozlz4(&s.file).map(|j| from_firefox(&j)).unwrap_or_default()
    } else {
        std::fs::read_to_string(&s.file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|j| from_chromium(&j, s.name))
            .unwrap_or_default()
    };
    finish(s.name, nodes);
}

fn finish(from: &str, nodes: Vec<Node>) {
    let total: usize = nodes.iter().map(|n| if let Node::Folder { items, .. } = n { items.len() } else { 1 }).sum();
    let added = crate::saved::add_imported(nodes);
    util::log(&format!("imported {added} of {total} bookmarks from {from}"));
    let title = if total == 0 {
        format!("No bookmarks found in {from}")
    } else {
        format!("✓  Imported {added} bookmarks from {from}")
    };
    let detail = if added < total { format!("{} were already saved", total - added) } else { "They are in the saved tabs bar".to_string() };
    crate::backup::show_notice(&title, &detail);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz4_literals_and_match() {
        // "abc" literal + match (offset 3, length 6) => "abcabcabc"
        let block = [0x32, b'a', b'b', b'c', 3, 0];
        assert_eq!(lz4_block(&block, 9).unwrap(), b"abcabcabc");
        // literal only
        assert_eq!(lz4_block(&[0x50, b'h', b'e', b'l', b'l', b'o'], 5).unwrap(), b"hello");
    }

    #[test]
    fn html_export() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
  <DT><H3>Bookmarks bar</H3>
  <DL><p>
    <DT><A HREF="https://a.com/">A site</A>
    <DT><H3>Work</H3>
    <DL><p>
      <DT><A HREF="https://b.com/x?y=1&amp;z=2">B &amp; co</A>
      <DT><H3>Deeper</H3>
      <DL><p><DT><A HREF="https://c.com/">C</A></DL><p>
    </DL><p>
    <DT><A HREF="https://d.com/">D</A>
  </DL><p>
</DL><p>"#;
        let nodes = from_html(html);
        let desc: Vec<String> = nodes
            .iter()
            .map(|n| match n {
                Node::Link(l) => format!("{}={}", l.title, l.url),
                Node::Folder { title, items } => format!("[{title}:{}]", items.iter().map(|l| l.url.as_str()).collect::<Vec<_>>().join(",")),
            })
            .collect();
        assert_eq!(desc, vec!["A site=https://a.com/", "[Work:https://b.com/x?y=1&z=2,https://c.com/]", "D=https://d.com/"]);
    }

    #[test]
    fn chromium_json() {
        let j: Value = serde_json::from_str(r#"{"roots":{
            "bookmark_bar":{"children":[
                {"type":"url","name":"A","url":"https://a.com/"},
                {"type":"folder","name":"F","children":[{"type":"url","name":"B","url":"https://b.com/"},
                    {"type":"folder","name":"G","children":[{"type":"url","name":"C","url":"https://c.com/"}]}]}]},
            "other":{"children":[{"type":"url","name":"O","url":"https://o.com/"}]},
            "synced":{"children":[]}}}"#).unwrap();
        let nodes = from_chromium(&j, "Edge");
        assert_eq!(nodes.len(), 3);
        assert!(matches!(&nodes[1], Node::Folder { title, items } if title == "F" && items.len() == 2));
        assert!(matches!(&nodes[2], Node::Folder { title, .. } if title == "Edge - other bookmarks"));
    }
}
