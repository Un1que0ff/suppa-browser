// Chrome extensions via WebView2's browser-extension API.
//
//  * Install from the Chrome Web Store: open the extension's page, then
//    Ext -> "Install extension from this page". Suppa downloads the .crx,
//    unpacks it and loads it.
//  * Load unpacked: pick a folder that contains manifest.json.
//  * The extension's popup / options page opens in a tab.
use std::path::{Path, PathBuf};

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler,
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Shell::{FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
use windows::core::{BOOL, HSTRING, Interface, PWSTR};

use crate::app::{self, *};
use crate::{settings, tabs, util};

const WEB_STORE: &str = "https://chromewebstore.google.com/";

fn profile() -> Option<ICoreWebView2Profile7> {
    // Extensions live in the normal profile, never the incognito one.
    let wv = app::with(|s| s.tabs.iter().filter(|t| !t.incognito).find_map(|t| t.webview.clone())).flatten()?;
    unsafe { wv.cast::<ICoreWebView2_13>().ok()?.Profile().ok()?.cast::<ICoreWebView2Profile7>().ok() }
}

/// Called whenever a WebView is created; loads the extension list once.
pub fn on_webview_ready(_wv: &ICoreWebView2) {
    let first = app::with(|s| !std::mem::replace(&mut s.extensions_loaded, true)).unwrap_or(false);
    if first {
        refresh();
    }
}

pub fn refresh() {
    let Some(p) = profile() else { return };
    let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(|res, list| {
        let (Ok(()), Some(list)) = (res, list) else { return Ok(()) };
        let mut exts = Vec::new();
        unsafe {
            let mut n = 0u32;
            let _ = list.Count(&mut n);
            for i in 0..n {
                let Ok(e) = list.GetValueAtIndex(i) else { continue };
                let mut id = PWSTR::null();
                let mut name = PWSTR::null();
                let mut en = BOOL(0);
                let _ = e.Id(&mut id);
                let _ = e.Name(&mut name);
                let _ = e.IsEnabled(&mut en);
                let (id, name) = (util::take_pwstr(id), util::take_pwstr(name));
                // Hide Edge's built-in component extensions.
                if name == "Microsoft Clipboard Extension" || name == "Microsoft Edge PDF Viewer" {
                    continue;
                }
                exts.push(Ext { id, name, enabled: en.as_bool(), handle: e });
            }
        }
        exts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        app::with(|s| s.extensions = exts);
        toolbar_sync();
        Ok(())
    }));
    unsafe {
        let _ = p.GetBrowserExtensions(&handler);
    }
}

/// Reads popup and options page paths from an extension's manifest.json.
fn manifest_pages(folder: &Path) -> (Option<String>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(folder.join("manifest.json")) else { return (None, None) };
    let text = text.trim_start_matches('\u{feff}');
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return (None, None) };
    let s = |p: &[&str]| -> Option<String> {
        let mut cur = &v;
        for k in p {
            cur = cur.get(*k)?;
        }
        cur.as_str().map(|x| x.trim_start_matches('/').to_string()).filter(|x| !x.is_empty())
    };
    let popup = s(&["action", "default_popup"])
        .or_else(|| s(&["browser_action", "default_popup"]))
        .or_else(|| s(&["page_action", "default_popup"]));
    let options = s(&["options_ui", "page"]).or_else(|| s(&["options_page"]));
    (popup, options)
}

// ---- Toolbar buttons (right of the address bar) -----------------------------------------

/// The extension's toolbar icon from manifest.json, as a data: URL.
fn manifest_icon(folder: &Path) -> Option<String> {
    use base64::Engine;
    let text = std::fs::read_to_string(folder.join("manifest.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let pick = |x: &serde_json::Value| -> Option<String> {
        if let Some(s) = x.as_str() {
            return Some(s.to_string());
        }
        // {"16": "...", "32": "...", ...}: the size closest to 32
        let obj = x.as_object()?;
        let mut best: Option<(i64, String)> = None;
        for (k, p) in obj {
            let size: i64 = k.parse().unwrap_or(0);
            let score = (size - 32).abs();
            if let Some(p) = p.as_str() {
                if best.as_ref().map(|(b, _)| score < *b).unwrap_or(true) {
                    best = Some((score, p.to_string()));
                }
            }
        }
        best.map(|(_, p)| p)
    };
    let path = v["action"]
        .get("default_icon")
        .and_then(pick)
        .or_else(|| v["browser_action"].get("default_icon").and_then(pick))
        .or_else(|| v.get("icons").and_then(pick))?;
    let file = folder.join(path.trim_start_matches('/'));
    let bytes = std::fs::read(&file).ok()?;
    let mime = match file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Sends the toolbar buttons (enabled extensions that aren't hidden) to the bar.
pub fn toolbar_sync() {
    let list: Vec<(String, String, Option<String>)> = app::with(|s| {
        s.extensions
            .iter()
            .filter(|e| e.enabled && !s.settings.ext_hidden.contains(&e.id))
            .map(|e| (e.id.clone(), e.name.clone(), s.settings.ext_folder(&e.id)))
            .collect()
    })
    .unwrap_or_default();
    let items: Vec<serde_json::Value> = list
        .into_iter()
        .filter_map(|(id, name, folder)| {
            let folder = folder?;
            let icon = manifest_icon(Path::new(&folder)).unwrap_or_default();
            Some(serde_json::json!({ "id": id, "name": name, "icon": icon }))
        })
        .collect();
    crate::ui::post(serde_json::json!({ "type": "exts", "items": items }).to_string());
}

fn ext_by_id(id: &str) -> Option<(String, Option<String>, Option<String>, ICoreWebView2BrowserExtension)> {
    app::with(|s| {
        let e = s.extensions.iter().find(|e| e.id == id)?;
        let folder = s.settings.ext_folder(id);
        let (popup, options) = folder.as_ref().map(|f| manifest_pages(Path::new(f))).unwrap_or((None, None));
        Some((e.name.clone(), popup, options, e.handle.clone()))
    })
    .flatten()
}

/// Toolbar button clicked: the extension's popup in a small window under it.
pub fn toolbar_click(id: &str, anchor: windows::Win32::Foundation::POINT) {
    let Some((name, popup, options, _)) = ext_by_id(id) else { return };
    match (popup, options) {
        (Some(p), _) => crate::extpopup::open(&format!("chrome-extension://{id}/{p}"), anchor),
        (None, Some(o)) => {
            tabs::new_tab(Some(format!("chrome-extension://{id}/{o}")), true);
        }
        (None, None) => util::info(app::hwnd(), &format!("{name}\n\nThis extension has no popup or options page; it works on its own on web pages.")),
    }
}

/// Right-click on a toolbar button.
pub fn toolbar_menu(id: &str, anchor: windows::Win32::Foundation::POINT) {
    use crate::popup::Item;
    let Some((name, popup, options, _)) = ext_by_id(id) else { return };
    let items = vec![
        Item::new(0, name).enabled(false),
        Item::sep(),
        Item::new(1, "Open popup").enabled(popup.is_some()),
        Item::new(2, "Options").enabled(options.is_some()),
        Item::sep(),
        Item::new(3, "Hide from toolbar"),
        Item::new(4, "Remove extension..."),
    ];
    let id = id.to_string();
    crate::popup::show("ext-button", items, anchor, false, move |pick| match pick {
        Some(1) => toolbar_click(&id, anchor),
        Some(2) => {
            if let Some((_, _, Some(o), _)) = ext_by_id(&id) {
                tabs::new_tab(Some(format!("chrome-extension://{id}/{o}")), true);
            }
        }
        Some(3) => set_hidden(&id, true),
        Some(4) => {
            if let Some((_, _, _, handle)) = ext_by_id(&id) {
                let folder = app::with(|s| s.settings.ext_folder(&id)).flatten();
                remove(id.clone(), handle, folder);
            }
        }
        _ => {}
    });
}

fn set_hidden(id: &str, hidden: bool) {
    app::with(|s| {
        s.settings.ext_hidden.retain(|x| x != id);
        if hidden {
            s.settings.ext_hidden.push(id.to_string());
        }
        settings::save(&s.settings);
    });
    toolbar_sync();
}

pub fn show_menu() {
    crate::popup::show("extensions", menu_items(), crate::ui::menu_anchor(), true, |pick| {
        if let Some(id) = pick {
            if id > 0 {
                crate::window::command(id as u16);
            }
        }
    });
}

/// The extensions menu (also used as a submenu of the ≡ menu).
pub fn menu_items() -> Vec<crate::popup::Item> {
    use crate::popup::Item;
    let items: Vec<(String, bool, Option<String>, bool)> = app::with(|s| {
        s.extensions
            .iter()
            .map(|e| (e.name.clone(), e.enabled, s.settings.ext_folder(&e.id), !s.settings.ext_hidden.contains(&e.id)))
            .collect()
    })
    .unwrap_or_default();
    let mut out = Vec::new();
    for (i, (name, enabled, folder, shown)) in items.iter().enumerate() {
        let base = CMD_EXT_BASE + (i * 8) as u16;
        let (popup, options) = folder.as_ref().map(|f| manifest_pages(Path::new(f))).unwrap_or((None, None));
        let label = if *enabled { name.clone() } else { format!("{name} (off)") };
        out.push(Item::sub(
            label,
            vec![
                Item::cmd(base, "Open popup in tab").enabled(popup.is_some() && *enabled),
                Item::cmd(base + 1, "Options").enabled(options.is_some() && *enabled),
                Item::cmd(base + 2, "Enabled").checked(*enabled),
                Item::cmd(base + 4, "Show in toolbar").checked(*shown).enabled(folder.is_some()),
                Item::sep(),
                Item::cmd(base + 3, "Remove"),
            ],
        ));
    }
    if !out.is_empty() {
        out.push(Item::sep());
    }
    out.push(Item::cmd(CMD_EXT_INSTALL_STORE, "Install extension from this Web Store page"));
    out.push(Item::cmd(CMD_EXT_OPEN_STORE, "Open Chrome Web Store"));
    out.push(Item::cmd(CMD_EXT_LOAD_UNPACKED, "Load unpacked extension folder..."));
    out.push(Item::cmd(CMD_EXT_OPEN_FOLDER, "Open extensions folder"));
    out
}

pub fn command(cmd: u16) {
    match cmd {
        CMD_EXT_INSTALL_STORE => install_from_current_page(),
        CMD_EXT_OPEN_STORE => {
            tabs::new_tab(Some(WEB_STORE.to_string()), true);
        }
        CMD_EXT_LOAD_UNPACKED => {
            if let Some(folder) = pick_folder() {
                add_folder(folder, false);
            }
        }
        CMD_EXT_OPEN_FOLDER => util::open_in_explorer(&util::sub_dir("extensions")),
        c if c >= CMD_EXT_BASE => {
            let i = ((c - CMD_EXT_BASE) / 8) as usize;
            let action = (c - CMD_EXT_BASE) % 8;
            let r = app::with(|s| s.extensions.get(i).map(|e| (e.id.clone(), e.enabled, e.handle.clone(), s.settings.ext_folder(&e.id))))
                .flatten();
            let Some((id, enabled, handle, folder)) = r else { return };
            if action == 4 {
                let hidden = app::with(|s| s.settings.ext_hidden.contains(&id)).unwrap_or(false);
                set_hidden(&id, !hidden);
                return;
            }
            match action {
                0 | 1 => {
                    let (popup, options) = folder.as_ref().map(|f| manifest_pages(Path::new(f))).unwrap_or((None, None));
                    let page = if action == 0 { popup } else { options };
                    if let Some(page) = page {
                        tabs::new_tab(Some(format!("chrome-extension://{id}/{page}")), true);
                    }
                }
                2 => unsafe {
                    let _ = handle.Enable(
                        !enabled,
                        &BrowserExtensionEnableCompletedHandler::create(Box::new(|_| {
                            refresh();
                            Ok(())
                        })),
                    );
                },
                3 => remove(id, handle, folder),
                _ => {}
            }
        }
        _ => {}
    }
}

fn remove(id: String, handle: ICoreWebView2BrowserExtension, folder: Option<String>) {
    let name = app::with(|s| s.extensions.iter().find(|e| e.id == id).map(|e| e.name.clone())).flatten().unwrap_or_default();
    crate::popup::confirm(&format!("Remove \"{name}\"?"), "The extension and its settings are removed from Suppa.", "Remove", "Cancel", move || {
        remove_now(id, handle, folder)
    });
}

fn remove_now(id: String, handle: ICoreWebView2BrowserExtension, folder: Option<String>) {
    util::log(&format!("extension removed by the user: {id}"));
    unsafe {
        let _ = handle.Remove(&BrowserExtensionRemoveCompletedHandler::create(Box::new(move |_| {
            // Delete the files only if Suppa unpacked them itself.
            if let Some(f) = &folder {
                let p = PathBuf::from(f);
                if p.starts_with(util::sub_dir("extensions")) {
                    let _ = std::fs::remove_dir_all(&p);
                }
            }
            app::with(|s| {
                s.settings.ext_folders.retain(|(i, _)| *i != id);
                settings::save(&s.settings);
            });
            refresh();
            Ok(())
        })));
    }
}

pub fn pick_folder_titled(title: &str) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().ok()?;
        dlg.SetOptions(opts | FOS_PICKFOLDERS).ok()?;
        let _ = dlg.SetTitle(&HSTRING::from(title));
        dlg.Show(Some(app::hwnd())).ok()?;
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

fn pick_folder() -> Option<PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().ok()?;
        dlg.SetOptions(opts | FOS_PICKFOLDERS).ok()?;
        let _ = dlg.SetTitle(&HSTRING::from("Select the unpacked extension folder (contains manifest.json)"));
        dlg.Show(Some(app::hwnd())).ok()?;
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// Registers an unpacked extension folder with WebView2.
fn add_folder(folder: PathBuf, from_store: bool) {
    let hwnd = app::hwnd();
    if !folder.join("manifest.json").exists() {
        util::info(hwnd, "That folder has no manifest.json, so it is not an extension.");
        return;
    }
    let Some(p) = profile() else {
        util::info(hwnd, "Open any web page first, then try again.");
        return;
    };
    let folder_s = folder.to_string_lossy().to_string();
    let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |res, ext| {
        match (res, ext) {
            (Ok(()), Some(e)) => {
                let mut id = PWSTR::null();
                let mut name = PWSTR::null();
                unsafe {
                    let _ = e.Id(&mut id);
                    let _ = e.Name(&mut name);
                }
                let (id, name) = (util::take_pwstr(id), util::take_pwstr(name));
                app::with(|s| {
                    s.settings.ext_folders.retain(|(i, _)| *i != id);
                    s.settings.ext_folders.push((id.clone(), folder_s.clone()));
                    settings::save(&s.settings);
                });
                refresh();
                util::log(&format!("extension installed: {name} ({id})"));
                let _ = hwnd;
                crate::popup::show(
                    "ext-done",
                    vec![
                        crate::popup::Item::new(0, format!("✓  Added \"{name}\" to Suppa")).enabled(false),
                        crate::popup::Item::new(0, "Open its popup or options from ≡ → Extensions").enabled(false),
                        crate::popup::Item::sep(),
                        crate::popup::Item::new(1, "OK"),
                    ],
                    crate::ui::corner_anchor(),
                    true,
                    |_| {},
                );
            }
            (r, _) => {
                if from_store {
                    let _ = std::fs::remove_dir_all(&folder_s);
                }
                util::info(
                    hwnd,
                    &format!(
                        "This extension could not be loaded\n\nSome Chrome extensions use features WebView2 doesn't support yet. ({:?})",
                        r.err()
                    ),
                );
            }
        }
        Ok(())
    }));
    unsafe {
        let _ = p.AddBrowserExtension(&HSTRING::from(folder.as_os_str()), &handler);
    }
}

/// Extracts the 32-char extension id (a-p) from a Chrome Web Store URL.
fn store_id(url: &str) -> Option<String> {
    if !(url.contains("chromewebstore.google.com") || url.contains("chrome.google.com/webstore")) {
        return None;
    }
    // The id may also be inside a redirect parameter (e.g. Google's consent page),
    // so scan the whole URL for a standalone 32-letter a-p token.
    url.split(|c: char| !c.is_ascii_lowercase())
        .find(|seg| seg.len() == 32 && seg.bytes().all(|b| (b'a'..=b'p').contains(&b)))
        .map(str::to_string)
}

fn install_from_current_page() {
    let r = app::with(|s| (s.hwnd, s.active_tab().map(|t| t.url.clone()).unwrap_or_default()));
    let Some((hwnd, url)) = r else { return };
    let Some(id) = store_id(&url) else {
        util::info(
            hwnd,
            "Open the extension's page on the Chrome Web Store first\n(chromewebstore.google.com/detail/...), then choose this again.",
        );
        return;
    };
    install_id(id);
}

/// "Add to Suppa" clicked on the Chrome Web Store: themed confirmation first.
pub fn ask_install(id: String, name: String, anchor: windows::Win32::Foundation::POINT) {
    use crate::popup::{self, Item};
    let valid = id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b));
    if !valid {
        return;
    }
    let name: String = name.chars().filter(|c| !c.is_control()).take(80).collect();
    let name = if name.trim().is_empty() { "this extension".to_string() } else { format!("\"{}\"", name.trim()) };
    // Installed copies get a new id; their folder is named after the store id.
    let already = app::with(|s| {
        s.extensions.iter().any(|e| {
            e.id == id
                || s.settings.ext_folder(&e.id).map(|f| Path::new(&f).file_name().map(|n| n == id.as_str()).unwrap_or(false)).unwrap_or(false)
        })
    })
    .unwrap_or(false);
    let items = if already {
        vec![
            Item::new(0, format!("{name} is already added to Suppa")).enabled(false),
            Item::sep(),
            Item::new(2, "OK"),
        ]
    } else {
        vec![
            Item::new(0, format!("Add {name} to Suppa?")).enabled(false),
            Item::new(0, "It can read and change data on the websites you visit").enabled(false),
            Item::sep(),
            Item::new(1, "Add extension").hot(true),
            Item::new(2, "Cancel"),
        ]
    };
    popup::show("ext-install", items, anchor, false, move |pick| {
        if pick == Some(1) {
            install_id(id);
        }
    });
}

/// Downloads the extension from the Chrome Web Store and installs it.
pub fn install_id(id: String) {
    let r = app::with(|s| (s.hwnd, s.browser_version.clone()));
    let Some((hwnd, version)) = r else { return };
    util::log(&format!("installing extension {id}"));
    let version = if version.is_empty() { "130.0.0.0".to_string() } else { version };
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let result = download_and_unpack(&id, &version);
        let boxed = Box::new(result);
        util::post(HWND(hwnd_raw as *mut _), WM_APP_EXT_UNPACKED, 0, Box::into_raw(boxed) as isize);
    });
}

/// UI-thread side of the store install (after the background download).
pub fn on_unpacked(lparam: isize) {
    if lparam == 0 {
        return;
    }
    let result = unsafe { Box::from_raw(lparam as *mut Result<PathBuf, String>) };
    match *result {
        Ok(folder) => add_folder(folder, true),
        Err(e) => {
            util::info(app::hwnd(), &format!("Extension install failed\n\n{e}"));
        }
    }
}

fn download_and_unpack(id: &str, version: &str) -> Result<PathBuf, String> {
    let dir = util::sub_dir("extensions");
    let crx = dir.join(format!("{id}.crx"));
    let url = format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&prodversion={version}&acceptformat=crx2,crx3&x=id%3D{id}%26installsource%3Dondemand%26uc"
    );
    util::download(&url, &crx)?;
    let bytes = std::fs::read(&crx).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&crx);
    let zip = crx_to_zip(&bytes).ok_or("Downloaded file is not a valid extension package")?;
    let zip_path = dir.join(format!("{id}.zip"));
    std::fs::write(&zip_path, zip).map_err(|e| e.to_string())?;

    let out = dir.join(id);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    // Windows 10+ ships bsdtar, which extracts zip files.
    let tar = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32\\tar.exe");
    use std::os::windows::process::CommandExt;
    let status = std::process::Command::new(tar)
        .arg("-xf")
        .arg(&zip_path)
        .arg("-C")
        .arg(&out)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .map_err(|e| format!("could not run tar: {e}"))?;
    let _ = std::fs::remove_file(&zip_path);
    if !status.success() || !out.join("manifest.json").exists() {
        let _ = std::fs::remove_dir_all(&out);
        return Err("Could not unpack the extension".into());
    }
    // Chrome refuses unpacked extensions containing the store's _metadata folder.
    let _ = std::fs::remove_dir_all(out.join("_metadata"));
    Ok(out)
}

/// Strips the CRX2/CRX3 header and returns the inner zip.
fn crx_to_zip(b: &[u8]) -> Option<&[u8]> {
    if b.starts_with(b"PK") {
        return Some(b);
    }
    if b.len() < 16 || &b[0..4] != b"Cr24" {
        return None;
    }
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
    let start = match u32_at(4) {
        3 => 12 + u32_at(8),
        2 => 16 + u32_at(8) + u32_at(12),
        _ => return None,
    };
    b.get(start..)
}
