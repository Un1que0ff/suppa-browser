// Updates from GitHub releases (github.com/Un1que0ff/suppa-browser).
//   * once a day (Settings -> Update and files -> "Check for updates
//     automatically") and from ≡ -> Check for updates, Suppa asks GitHub for
//     the latest release. Only the version is compared; nothing about you is sent.
//   * Suppa installed with SuppaSetup.exe: "Update now" downloads the new setup
//     and runs it silently; it closes Suppa (tabs are saved), replaces the files
//     and starts Suppa again.
//   * A portable copy (zip) or a development build: the download page opens.
//   * After an update, "What's new" shows that version's release notes from GitHub.
use std::sync::Mutex;

use crate::app::{self, *};
use crate::util;

const REPO_API: &str = "https://api.github.com/repos/Un1que0ff/suppa-browser/releases";
const RELEASES: &str = "https://github.com/Un1que0ff/suppa-browser/releases";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const LAST_CHECK: &str = "update-check.txt";
const LAST_VERSION: &str = "last-version.txt";

#[derive(Clone)]
struct Release {
    version: String,
    notes: String,
    setup_url: String,
    page: String,
}

/// Background thread -> UI thread.
static CHECKED: Mutex<Option<(bool, Result<Release, String>)>> = Mutex::new(None);
static NOTES: Mutex<Option<Result<Release, String>>> = Mutex::new(None);

thread_local! {
    static SHOW_NEWS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn post(cmd: u16, hwnd: isize) {
    util::post(windows::Win32::Foundation::HWND(hwnd as _), WM_APP_COMMAND, cmd as usize, 0);
}

/// "0.10.0" -> (0, 10, 0)
fn parse(v: &str) -> (u32, u32, u32) {
    let mut p = v.trim().trim_start_matches('v').split('.').map(|x| x.parse().unwrap_or(0));
    (p.next().unwrap_or(0), p.next().unwrap_or(0), p.next().unwrap_or(0))
}

fn fetch(url: &str) -> Result<Release, String> {
    let tmp = util::data_dir().join("update-info.json");
    // Unique address: Windows' download cache never hands back an old answer.
    util::download(&format!("{url}?t={}", util::unix_now()), &tmp)?;
    let text = std::fs::read_to_string(&tmp).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&tmp);
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("GitHub answer: {e}"))?;
    let tag = v["tag_name"].as_str().ok_or("GitHub sent no version")?;
    let setup_url = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == "SuppaSetup.exe"))
        .and_then(|x| x["browser_download_url"].as_str())
        .unwrap_or("")
        .to_string();
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        setup_url,
        page: v["html_url"].as_str().unwrap_or(RELEASES).to_string(),
    })
}

// ---- startup ----------------------------------------------------------------------------

/// Very early at startup (before settings are written): was Suppa updated
/// since the last run? Then "What's new" is shown once the window is ready.
pub fn note_version() {
    let path = util::data_dir().join(LAST_VERSION);
    let last = std::fs::read_to_string(&path).ok();
    let used_before = last.is_some() || util::data_dir().join("settings.ini").exists();
    let updated = match &last {
        Some(l) => parse(l) < parse(VERSION),
        None => used_before, // first version that keeps track: an update too
    };
    if updated {
        SHOW_NEWS.with(|s| s.set(true));
    }
    let _ = std::fs::write(&path, VERSION);
}

/// The window is ready: "What's new" after an update, and the daily check.
pub fn on_ready() {
    if SHOW_NEWS.with(|s| s.replace(false)) {
        whats_new();
    }
    let on = app::with(|s| s.settings.auto_update && !s.secondary).unwrap_or(false);
    if !on {
        return;
    }
    let path = util::data_dir().join(LAST_CHECK);
    let last: u64 = std::fs::read_to_string(&path).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
    if util::unix_now().saturating_sub(last) >= 24 * 3600 {
        let _ = std::fs::write(&path, util::unix_now().to_string());
        check(false);
    }
}

// ---- check ------------------------------------------------------------------------------

/// Asks GitHub for the latest release. `manual`: also say "up to date" / errors.
pub fn check(manual: bool) {
    let hwnd = app::hwnd().0 as isize;
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let r = fetch(&format!("{REPO_API}/latest"));
        if let Err(e) = &r {
            util::log(&format!("update check: {e}"));
        }
        *CHECKED.lock().unwrap() = Some((manual, r));
        post(CMD_UPDATE_FOUND, hwnd);
    });
}

/// Installed with the setup in this folder (so the setup can update it)?
fn installed_here() -> bool {
    let exe_dir = crate::scripts::exe_dir();
    let reg = reg_install_location().map(std::path::PathBuf::from);
    match (reg.and_then(|p| p.canonicalize().ok()), exe_dir.canonicalize().ok()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn reg_install_location() -> Option<String> {
    use windows::Win32::System::Registry::*;
    use windows::core::w;
    unsafe {
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\SuppaBrowser"),
            w!("InstallLocation"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let n = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

thread_local! {
    static FOUND: std::cell::RefCell<Option<Release>> = const { std::cell::RefCell::new(None) };
}

/// CMD_UPDATE_FOUND: the answer from GitHub.
pub fn on_checked() {
    let Some((manual, r)) = CHECKED.lock().unwrap().take() else { return };
    let rel = match r {
        Ok(rel) => rel,
        Err(e) => {
            if manual {
                crate::popup::message(&format!("Could not check for updates\n\n{e}"));
            }
            return;
        }
    };
    util::log(&format!("update check: latest is {}", rel.version));
    if parse(&rel.version) <= parse(VERSION) {
        if manual {
            crate::popup::message(&format!("Suppa is up to date\n\nYou have the newest version, {VERSION}."));
        }
        return;
    }
    util::log(&format!("update: {} available", rel.version));
    let summary = summary(&rel.notes, 6).join("\n");
    FOUND.with(|f| *f.borrow_mut() = Some(rel.clone()));
    if installed_here() && !rel.setup_url.is_empty() {
        crate::popup::confirm(
            &format!("Suppa {} is available", rel.version),
            &format!("{summary}\n\nSuppa downloads it, closes, updates itself and opens again with your tabs."),
            "Update now",
            "Later",
            install,
        );
    } else {
        let page = rel.page.clone();
        crate::popup::confirm(
            &format!("Suppa {} is available", rel.version),
            &format!("{summary}\n\nThis copy of Suppa was not installed with the setup, so it cannot update itself. Download the new version?"),
            "Open download page",
            "Later",
            move || {
                let _ = crate::tabs::new_tab(Some(page), true);
            },
        );
    }
}

/// "Update now": download the setup, run it silently for this folder.
pub fn install() {
    let Some(rel) = FOUND.with(|f| f.borrow().clone()) else {
        check(true);
        return;
    };
    let hwnd = app::hwnd().0 as isize;
    let dir = crate::scripts::exe_dir();
    crate::backup::show_notice("Downloading the update…", &format!("Suppa {} - Suppa restarts when it is ready.", rel.version));
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let setup = util::data_dir().join("SuppaSetup-update.exe");
        let r = util::download(&rel.setup_url, &setup).and_then(|_| {
            // A real Windows program of a sensible size, not an error page.
            let b = std::fs::read(&setup).map_err(|e| e.to_string())?;
            if b.len() < 500_000 || !b.starts_with(b"MZ") {
                return Err("the downloaded file is not the Suppa setup".into());
            }
            Ok(())
        });
        match r {
            Ok(()) => {
                util::log(&format!("update: running the setup for {}", rel.version));
                // The setup closes this Suppa (tabs are saved), updates and starts it again.
                let _ = std::process::Command::new(&setup).arg("/S").arg(format!("/D={}", dir.display())).spawn();
            }
            Err(e) => {
                util::log(&format!("update failed: {e}"));
                *CHECKED.lock().unwrap() = Some((true, Err(format!("The update could not be downloaded: {e}"))));
                post(CMD_UPDATE_FOUND, hwnd);
            }
        }
    });
}

// ---- What's new ---------------------------------------------------------------------------

/// Shows the release notes of this version (from GitHub).
pub fn whats_new() {
    let hwnd = app::hwnd().0 as isize;
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        *NOTES.lock().unwrap() = Some(fetch(&format!("{REPO_API}/tags/v{VERSION}")));
        post(CMD_NOTES_READY, hwnd);
    });
}

/// CMD_NOTES_READY: the notes arrived.
pub fn on_notes() {
    use crate::popup::Item;
    let Some(r) = NOTES.lock().unwrap().take() else { return };
    let (lines, page) = match r {
        Ok(rel) => (summary(&rel.notes, 16), rel.page),
        Err(e) => {
            util::log(&format!("what's new: {e}"));
            (vec![format!("Suppa was updated to {VERSION}.")], RELEASES.to_string())
        }
    };
    let mut items = vec![Item::heading(format!("What's new in Suppa {VERSION}"))];
    for l in lines {
        items.push(Item::text(l));
    }
    items.push(Item::sep());
    items.push(Item::new(1, "OK").hot(true));
    items.push(Item::new(2, "Full release notes"));
    crate::popup::show("dialog-news", items, windows::Win32::Foundation::POINT::default(), false, move |pick| {
        if pick == Some(2) {
            let _ = crate::tabs::new_tab(Some(page), true);
        }
    });
}

/// Release notes (Markdown) -> short plain lines: headings and bullet points,
/// without pictures, links and the downloads part.
fn summary(md: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in md.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("![") || l.starts_with("**Suppa Browser") {
            continue;
        }
        if l.starts_with('#') {
            let h = l.trim_start_matches('#').trim();
            if h.eq_ignore_ascii_case("downloads") {
                break;
            }
            out.push(plain(h).to_uppercase());
        } else if let Some(b) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
            out.push(format!("•  {}", plain(b)));
        } else if let Some(n) = l.split_once(". ").filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())) {
            out.push(format!("{}.  {}", n.0, plain(n.1)));
        } else {
            out.push(plain(l));
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Strips Markdown marks: **bold**, `code`, [text](link).
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        out += &rest[..i];
        let after = &rest[i + 1..];
        match after.find("](").and_then(|j| after[j + 2..].find(')').map(|k| (j, j + 2 + k))) {
            Some((j, end)) => {
                out += &after[..j];
                rest = &after[end + 1..];
            }
            None => {
                out.push('[');
                rest = after;
            }
        }
    }
    out += rest;
    out.replace("**", "").replace('`', "").replace(" -> ", " → ")
}
