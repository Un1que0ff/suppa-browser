// Saved tabs: the bar under the address bar (like bookmarks).
//  * Right-click a tab -> Save.
//  * Drag items to sort them. Drop one link on the middle of another to make a
//    folder with both; drop a link on a folder to move it inside.
//  * Right-click the bar for New folder, Rename, Delete...
// Stored in %LOCALAPPDATA%\SuppaBrowser\saved.json.
use std::cell::RefCell;

use serde_json::{Value, json};
use windows::Win32::Foundation::POINT;

use crate::app;
use crate::popup::{self, Item};
use crate::{tabs, ui, util};

#[derive(Clone)]
pub struct Link {
    pub title: String,
    pub url: String,
    pub icon: String,
}

pub enum Node {
    Link(Link),
    Folder { title: String, items: Vec<Link> },
}

thread_local! {
    static SAVED: RefCell<Option<Vec<Node>>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Vec<Node>) -> R) -> R {
    SAVED.with(|c| {
        let mut g = c.borrow_mut();
        f(g.get_or_insert_with(|| {
            LOADED_AT.with(|t| t.set(file_mtime()));
            load()
        }))
    })
}

thread_local! {
    static LOADED_AT: std::cell::Cell<u128> = const { std::cell::Cell::new(0) };
}

fn file_mtime() -> u128 {
    std::fs::metadata(file())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Window activated: another Suppa window may have changed saved.json.
pub fn reload_if_changed() {
    let loaded = SAVED.with(|c| c.borrow().is_some());
    if loaded && file_mtime() != LOADED_AT.with(|t| t.get()) {
        SAVED.with(|c| *c.borrow_mut() = None);
        sync();
    }
}

fn file() -> std::path::PathBuf {
    util::data_dir().join("saved.json")
}

fn link_from(v: &Value) -> Option<Link> {
    Some(Link {
        title: v.get("title")?.as_str().unwrap_or("").to_string(),
        url: v.get("url")?.as_str()?.to_string(),
        icon: v.get("icon").and_then(|i| i.as_str()).unwrap_or("").to_string(),
    })
}

fn load() -> Vec<Node> {
    let Ok(text) = std::fs::read_to_string(file()) else { return Vec::new() };
    let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { return Vec::new() };
    arr.iter()
        .filter_map(|v| {
            if let Some(title) = v.get("folder").and_then(|f| f.as_str()) {
                let items = v.get("items").and_then(|i| i.as_array()).map(|a| a.iter().filter_map(link_from).collect()).unwrap_or_default();
                Some(Node::Folder { title: title.to_string(), items })
            } else {
                link_from(v).map(Node::Link)
            }
        })
        .collect()
}

fn link_json(l: &Link) -> Value {
    json!({ "title": l.title, "url": l.url, "icon": l.icon })
}

fn save(v: &[Node]) {
    let arr: Vec<Value> = v
        .iter()
        .map(|n| match n {
            Node::Link(l) => link_json(l),
            Node::Folder { title, items } => json!({ "folder": title, "items": items.iter().map(link_json).collect::<Vec<_>>() }),
        })
        .collect();
    let text = serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".into());
    let tmp = file().with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, file());
    }
    LOADED_AT.with(|t| t.set(file_mtime()));
}

/// Sends the saved items to the bar.
pub fn sync() {
    let items: Vec<Value> = with(|v| {
        v.iter()
            .map(|n| match n {
                Node::Link(l) => json!({ "t": display_title(l), "u": l.url, "i": l.icon }),
                Node::Folder { title, items } => json!({ "f": title, "n": items.len() }),
            })
            .collect()
    });
    ui::post(json!({ "type": "saved", "items": items }).to_string());
    ui::request_sync(); // star in the address bar
}

/// Adds imported bookmarks (skips links already saved; a folder with the same
/// name gets the new links). Returns how many links were added.
pub fn add_imported(nodes: Vec<Node>) -> usize {
    let mut added = 0;
    with(|v| {
        let mut known: std::collections::HashSet<String> = v
            .iter()
            .flat_map(|n| match n {
                Node::Link(l) => vec![l.url.clone()],
                Node::Folder { items, .. } => items.iter().map(|l| l.url.clone()).collect(),
            })
            .collect();
        for n in nodes {
            match n {
                Node::Link(l) => {
                    if known.insert(l.url.clone()) {
                        v.push(Node::Link(l));
                        added += 1;
                    }
                }
                Node::Folder { title, items } => {
                    let fresh: Vec<Link> = items.into_iter().filter(|l| known.insert(l.url.clone())).collect();
                    if fresh.is_empty() {
                        continue;
                    }
                    added += fresh.len();
                    match v.iter_mut().find(|n| matches!(n, Node::Folder { title: t, .. } if *t == title)) {
                        Some(Node::Folder { items: existing, .. }) => existing.extend(fresh),
                        _ => v.push(Node::Folder { title, items: fresh }),
                    }
                }
            }
        }
        save(v);
    });
    sync();
    added
}

/// Star button: save the current website, or remove it if it is saved.
pub fn toggle_active() {
    let Some((idx, url)) = app::with(|s| (s.active, s.active_tab().map(|t| t.url.clone()).unwrap_or_default())) else {
        return;
    };
    if url.is_empty() {
        return;
    }
    if contains(&url) {
        with(|v| {
            v.retain(|n| !matches!(n, Node::Link(l) if l.url == url));
            for n in v.iter_mut() {
                if let Node::Folder { items, .. } = n {
                    items.retain(|l| l.url != url);
                }
            }
            save(v);
        });
        sync();
    } else {
        save_tab(idx);
    }
}

fn display_title(l: &Link) -> String {
    if l.title.trim().is_empty() { util::host_of(&l.url) } else { l.title.clone() }
}

pub fn contains(url: &str) -> bool {
    with(|v| {
        v.iter().any(|n| match n {
            Node::Link(l) => l.url == url,
            Node::Folder { items, .. } => items.iter().any(|l| l.url == url),
        })
    })
}

/// Tab right-click -> Save.
pub fn save_tab(index: usize) {
    let Some(Some(link)) = app::with(|s| {
        s.tabs.get(index).map(|t| Link {
            title: if t.title.is_empty() { util::host_of(&t.url) } else { t.title.clone() },
            url: t.url.clone(),
            icon: if t.favicon.starts_with("http") { t.favicon.clone() } else { String::new() },
        })
    }) else {
        return;
    };
    if link.url.is_empty() || contains(&link.url) {
        return;
    }
    with(|v| {
        v.push(Node::Link(link));
        save(v);
    });
    sync();
}

fn path_of(v: &Value) -> Vec<usize> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as usize)).collect()).unwrap_or_default()
}

fn get_link(path: &[usize]) -> Option<Link> {
    with(|v| match (path, v.get(*path.first()?)) {
        ([_], Some(Node::Link(l))) => Some(l.clone()),
        ([_, j], Some(Node::Folder { items, .. })) => items.get(*j).cloned(),
        _ => None,
    })
}

/// Removes and returns the item at `path` ([i] or [folder, j]).
fn take(v: &mut Vec<Node>, path: &[usize]) -> Option<Node> {
    match path {
        [i] if *i < v.len() => Some(v.remove(*i)),
        [i, j] => match v.get_mut(*i) {
            Some(Node::Folder { items, .. }) if *j < items.len() => Some(Node::Link(items.remove(*j))),
            _ => None,
        },
        _ => None,
    }
}

fn open(link: &Link, how: &str) {
    match how {
        "new" => {
            tabs::new_tab(Some(link.url.clone()), true);
        }
        "bg" => {
            tabs::new_tab(Some(link.url.clone()), false);
        }
        "inc" => tabs::new_incognito_url(link.url.clone()),
        _ => {
            tabs::navigate_active(&link.url);
            ui::focus_webview();
        }
    }
}

/// Messages from the bar (ui.js).
pub fn on_message(cmd: &str, v: &Value, anchor: POINT) {
    let path = path_of(v.get("path").unwrap_or(&Value::Null));
    match cmd {
        "saved_open" => {
            if let Some(l) = get_link(&path) {
                open(&l, v.get("how").and_then(|h| h.as_str()).unwrap_or("same"));
            }
        }
        "saved_folder" => {
            if let Some(&i) = path.first() {
                show_folder(i, anchor);
            }
        }
        "saved_menu" => show_menu(path, anchor),
        "saved_move" => {
            let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("before").to_string();
            let target = v.get("target").and_then(|t| t.as_u64()).unwrap_or(0) as usize;
            move_item(&path, &kind, target);
        }
        "saved_rename" => {
            let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
            if !title.is_empty() {
                rename(&path, title);
            }
            sync();
        }
        _ => {}
    }
}

fn rename(path: &[usize], title: String) {
    with(|v| {
        match path {
            [i] => match v.get_mut(*i) {
                Some(Node::Link(l)) => l.title = title,
                Some(Node::Folder { title: t, .. }) => *t = title,
                None => {}
            },
            [i, j] => {
                if let Some(Node::Folder { items, .. }) = v.get_mut(*i) {
                    if let Some(l) = items.get_mut(*j) {
                        l.title = title;
                    }
                }
            }
            _ => {}
        }
        save(v);
    });
}

/// Drag & drop result from the bar.
///  kind "before": put it at top-level position `target` (target = count = at the end)
///  kind "into":   into the folder at `target`
///  kind "merge":  dropped on the link at `target` -> new folder with both
fn move_item(from: &[usize], kind: &str, mut target: usize) {
    let mut new_folder = None;
    with(|v| {
        if from.len() == 1 && kind != "before" && from[0] == target {
            return;
        }
        // Removing a top-level item before the target shifts the target left.
        if from.len() == 1 && from[0] < target {
            target -= 1;
        }
        let Some(node) = take(v, from) else { return };
        match (kind, node) {
            ("into", Node::Link(l)) if matches!(v.get(target), Some(Node::Folder { .. })) => {
                if let Some(Node::Folder { items, .. }) = v.get_mut(target) {
                    items.push(l);
                }
            }
            ("merge", Node::Link(l)) if matches!(v.get(target), Some(Node::Link(_))) => {
                if let Node::Link(first) = v.remove(target) {
                    v.insert(target, Node::Folder { title: "New folder".into(), items: vec![first, l] });
                    new_folder = Some(target);
                }
            }
            ("merge", Node::Link(l)) if matches!(v.get(target), Some(Node::Folder { .. })) => {
                if let Some(Node::Folder { items, .. }) = v.get_mut(target) {
                    items.push(l);
                }
            }
            (_, node) => {
                let at = target.min(v.len());
                v.insert(at, node);
            }
        }
        save(v);
    });
    sync();
    if let Some(i) = new_folder {
        start_rename(&[i]);
    }
}

thread_local! {
    static PENDING_RENAME: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
}

/// Starts inline renaming in the bar. Posted, so it runs after a closing menu
/// has handed focus back to the window (otherwise the page would take focus).
fn start_rename(path: &[usize]) {
    PENDING_RENAME.with(|p| *p.borrow_mut() = Some(path.to_vec()));
    util::post(app::hwnd(), crate::app::WM_APP_COMMAND, crate::app::CMD_SAVED_RENAME as usize, 0);
}

pub fn run_pending_rename() {
    if let Some(path) = PENDING_RENAME.with(|p| p.borrow_mut().take()) {
        ui::focus_bar();
        ui::post(json!({ "type": "rename", "path": path }).to_string());
    }
}

fn new_folder(at: usize) {
    let at = with(|v| {
        let at = at.min(v.len());
        v.insert(at, Node::Folder { title: "New folder".into(), items: Vec::new() });
        save(v);
        at
    });
    sync();
    start_rename(&[at]);
}

// ---- Menus ---------------------------------------------------------------------------

/// Right-click on the saved-tabs bar (path empty = on free space).
fn show_menu(path: Vec<usize>, anchor: POINT) {
    let kind = with(|v| match path.first().and_then(|&i| v.get(i)) {
        Some(Node::Link(_)) => 1,
        Some(Node::Folder { .. }) => 2,
        None => 0,
    });
    let items = match kind {
        1 => vec![
            Item::new(1, "Open"),
            Item::new(2, "Open in new tab"),
            Item::new(3, "Open in incognito tab"),
            Item::sep(),
            Item::new(4, "Rename"),
            Item::new(5, "Delete"),
            Item::sep(),
            Item::new(6, "New folder"),
        ],
        2 => vec![
            Item::new(7, "Open all in new tabs"),
            Item::sep(),
            Item::new(4, "Rename folder"),
            Item::new(5, "Delete folder"),
            Item::sep(),
            Item::new(6, "New folder"),
        ],
        _ => vec![
            Item::new(8, "Save current tab"),
            Item::new(6, "New folder"),
            Item::sep(),
            Item::new(9, "Hide saved tabs bar"),
        ],
    };
    popup::show("saved", items, anchor, false, move |pick| {
        let Some(id) = pick else { return };
        match id {
            1 | 2 | 3 => {
                if let Some(l) = get_link(&path) {
                    open(&l, ["same", "new", "inc"][(id - 1) as usize]);
                }
            }
            4 => start_rename(&path),
            5 => {
                with(|v| {
                    take(v, &path);
                    save(v);
                });
                sync();
            }
            6 => {
                let at = path.first().map(|i| i + 1).unwrap_or(usize::MAX);
                new_folder(at);
            }
            7 => {
                if let Some(&i) = path.first() {
                    open_all(i);
                }
            }
            8 => {
                if let Some(a) = app::with(|s| s.active) {
                    save_tab(a);
                }
            }
            9 => crate::settings_page::set_saved_bar(false),
            _ => {}
        }
    });
}

fn open_all(folder: usize) {
    let links = with(|v| match v.get(folder) {
        Some(Node::Folder { items, .. }) => items.clone(),
        _ => Vec::new(),
    });
    for (k, l) in links.iter().enumerate() {
        tabs::new_tab(Some(l.url.clone()), k == 0);
    }
}

const OPEN_NEW: i64 = 100_000;
const OPEN_INC: i64 = 200_000;
const MOVE_OUT: i64 = 300_000;
const DELETE: i64 = 400_000;

fn folder_items(fi: usize) -> Option<Vec<Item>> {
    with(|v| match v.get(fi) {
        Some(Node::Folder { items, .. }) => {
            let mut out: Vec<Item> = items
                .iter()
                .enumerate()
                .map(|(j, l)| {
                    let j = j as i64;
                    Item::new(j, display_title(l)).icon(&l.icon).draggable().ctx(vec![
                        Item::new(OPEN_NEW + j, "Open in new tab"),
                        Item::new(OPEN_INC + j, "Open in incognito tab"),
                        Item::sep(),
                        Item::new(MOVE_OUT + j, "Move out of folder"),
                        Item::new(DELETE + j, "Delete"),
                    ])
                })
                .collect();
            if out.is_empty() {
                out.push(Item::new(-2, "Empty - drag saved tabs here").enabled(false));
            } else {
                out.push(Item::sep());
                out.push(Item::new(-1, "Open all in new tabs"));
            }
            Some(out)
        }
        _ => None,
    })
}

/// Click on a folder: its links in a menu (drag them to sort, right-click for more).
fn show_folder(fi: usize, anchor: POINT) {
    let Some(items) = folder_items(fi) else { return };
    popup::show_reorder(
        &format!("folder{fi}"),
        items,
        anchor,
        false,
        move |pick| {
            let Some(id) = pick else { return };
            match id {
                -1 => open_all(fi),
                j if (0..OPEN_NEW).contains(&j) => {
                    if let Some(l) = get_link(&[fi, j as usize]) {
                        open(&l, "same");
                    }
                }
                j if (OPEN_NEW..OPEN_INC).contains(&j) => {
                    if let Some(l) = get_link(&[fi, (j - OPEN_NEW) as usize]) {
                        open(&l, "new");
                    }
                }
                j if (OPEN_INC..MOVE_OUT).contains(&j) => {
                    if let Some(l) = get_link(&[fi, (j - OPEN_INC) as usize]) {
                        open(&l, "inc");
                    }
                }
                j if (MOVE_OUT..DELETE).contains(&j) => {
                    // Placed right after its folder in the bar.
                    move_item(&[fi, (j - MOVE_OUT) as usize], "before", fi + 1);
                }
                j if j >= DELETE => {
                    with(|v| {
                        take(v, &[fi, (j - DELETE) as usize]);
                        save(v);
                    });
                    sync();
                }
                _ => {}
            }
        },
        move |from, to| {
            if to == popup::DROPPED_OUTSIDE {
                // Let go outside the folder menu: if it's over the saved tabs bar,
                // the bar works out the exact place and sends saved_move.
                let mut pt = POINT::default();
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
                }
                if let Some((x, y)) = ui::screen_to_bar(pt) {
                    popup::close();
                    ui::post(json!({ "type": "external_drop", "path": [fi, from], "x": x, "y": y }).to_string());
                }
                return;
            }
            with(|v| {
                if let Some(Node::Folder { items, .. }) = v.get_mut(fi) {
                    if from < items.len() && to < items.len() {
                        let l = items.remove(from);
                        items.insert(to, l);
                    }
                }
                save(v);
            });
            if let Some(items) = folder_items(fi) {
                popup::refresh(items);
            }
            sync();
        },
    );
}
