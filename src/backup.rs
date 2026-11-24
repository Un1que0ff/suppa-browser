// Backup and restore of everything personal in Suppa:
//   settings.ini (theme, accent, bar sizes, window, ...), saved.json (saved
//   tabs), session.txt (open tabs), your scripts folder, unpacked extensions,
//   and the browser profile files with saved passwords, form autofill and
//   cookies (logins).
// Saved as one .zip. Passwords are encrypted by Windows for your user account,
// so they come back on this Windows account (not on another PC / user).
// Restore: the .zip is unpacked to a "restore" folder and applied at the next
// start, before the browser engine opens the files.
use std::path::{Path, PathBuf};


use crate::{app, scripts, util};

const MARKER: &str = "suppa-backup.txt";
/// Browser profile files (in UserData\EBWebView\...) that hold your data.
const PROFILE_FILES: [&str; 8] = [
    "Local State", // key that unlocks saved passwords
    "Default/Login Data",
    "Default/Login Data For Account",
    "Default/Web Data", // autofill: addresses, names
    "Default/Preferences",
    "Default/Secure Preferences",
    "Default/Extension Cookies",
    "Default/History",
];
/// Website cookies (logins), read/written through WebView2's cookie manager.
const COOKIES: &str = "cookies.json";

fn cookie_wv() -> Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2> {
    app::with(|s| s.tabs.iter().filter(|t| !t.incognito).find_map(|t| t.webview.clone())).flatten()
}

/// Reads every cookie of the normal profile as JSON (None if no page is open).
fn get_all_cookies(done: impl FnOnce(Option<String>) + 'static) {
    use webview2_com::GetCookiesCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::core::{BOOL, Interface, PWSTR, w};
    let Some(wv) = cookie_wv() else {
        done(None);
        return;
    };
    let Ok(cm) = (unsafe { wv.cast::<ICoreWebView2_2>().and_then(|w2| w2.CookieManager()) }) else {
        done(None);
        return;
    };
    let done = std::cell::RefCell::new(Some(done));
    let handler = GetCookiesCompletedHandler::create(Box::new(move |res, list| {
        let mut out = Vec::new();
        if let (Ok(()), Some(list)) = (res, list) {
            unsafe {
                let mut n = 0u32;
                let _ = list.Count(&mut n);
                for i in 0..n {
                    let Ok(c) = list.GetValueAtIndex(i) else { continue };
                    let s = |f: &dyn Fn(&mut PWSTR) -> windows::core::Result<()>| {
                        let mut p = PWSTR::null();
                        if f(&mut p).is_ok() { util::take_pwstr(p) } else { String::new() }
                    };
                    let (mut exp, mut http, mut sec, mut sess) = (0f64, BOOL(0), BOOL(0), BOOL(0));
                    let mut same = COREWEBVIEW2_COOKIE_SAME_SITE_KIND::default();
                    let _ = c.Expires(&mut exp);
                    let _ = c.IsHttpOnly(&mut http);
                    let _ = c.IsSecure(&mut sec);
                    let _ = c.IsSession(&mut sess);
                    let _ = c.SameSite(&mut same);
                    out.push(serde_json::json!({
                        "name": s(&|p| c.Name(p)), "value": s(&|p| c.Value(p)),
                        "domain": s(&|p| c.Domain(p)), "path": s(&|p| c.Path(p)),
                        "expires": exp, "httpOnly": http.as_bool(), "secure": sec.as_bool(),
                        "session": sess.as_bool(), "sameSite": same.0,
                    }));
                }
            }
        }
        if let Some(d) = done.borrow_mut().take() {
            d(Some(serde_json::Value::Array(out).to_string()));
        }
        Ok(())
    }));
    unsafe {
        if cm.GetCookies(w!(""), &handler).is_err() {
            // handler not called: write the backup without cookies
            util::log("backup: could not read cookies");
        }
    }
}

/// First page after a restore: put the backed-up cookies (logins) back.
pub fn import_pending_cookies(wv: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::core::{HSTRING, Interface};
    let file = util::data_dir().join("cookies-import.json");
    let Ok(text) = std::fs::read_to_string(&file) else { return };
    let _ = std::fs::remove_file(&file);
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else {
        util::log("restore: cookies file could not be read");
        return;
    };
    let Ok(cm) = (unsafe { wv.cast::<ICoreWebView2_2>().and_then(|w2| w2.CookieManager()) }) else { return };
    let mut n = 0;
    for c in &list {
        let g = |k: &str| c.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let b = |k: &str| c.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
        unsafe {
            let Ok(ck) = cm.CreateCookie(&HSTRING::from(g("name")), &HSTRING::from(g("value")), &HSTRING::from(g("domain")), &HSTRING::from(g("path")))
            else {
                continue;
            };
            if !b("session") {
                let _ = ck.SetExpires(c.get("expires").and_then(|v| v.as_f64()).unwrap_or(-1.0));
            }
            let _ = ck.SetIsHttpOnly(b("httpOnly"));
            let _ = ck.SetIsSecure(b("secure"));
            let same = c.get("sameSite").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let _ = ck.SetSameSite(COREWEBVIEW2_COOKIE_SAME_SITE_KIND(same));
            if cm.AddOrUpdateCookie(&ck).is_ok() {
                n += 1;
            }
        }
    }
    util::log(&format!("restore: {n} cookies (website logins) put back"));
}

fn profile_dir() -> PathBuf {
    util::sub_dir("UserData").join("EBWebView")
}

fn restore_dir() -> PathBuf {
    util::data_dir().join("restore")
}

fn tar() -> PathBuf {
    PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32\\tar.exe")
}

fn run_tar(args: &[&std::ffi::OsStr]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let st = std::process::Command::new(tar())
        .args(args)
        .creation_flags(0x0800_0000) // no console window
        .status()
        .map_err(|e| format!("could not run tar: {e}"))?;
    if st.success() { Ok(()) } else { Err(format!("tar failed ({st})")) }
}

fn copy_tree(from: &Path, to: &Path) -> usize {
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(from) else { return 0 };
    let _ = std::fs::create_dir_all(to);
    for e in rd.flatten() {
        let p = e.path();
        let dest = to.join(e.file_name());
        if p.is_dir() {
            n += copy_tree(&p, &dest);
        } else if std::fs::copy(&p, &dest).is_ok() {
            n += 1;
        }
    }
    n
}

/// ≡ -> Backup: asks where to save, writes the .zip.
pub fn backup() {
    let date = {
        let secs = util::unix_now();
        let days = secs / 86400;
        // civil date from days since 1970 (Howard Hinnant's algorithm)
        let z = days as i64 + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        format!("{y:04}-{m:02}-{d:02}")
    };
    let Some(out) = util::save_dialog("Save Suppa backup", &format!("Suppa backup {date}.zip"), "Zip archive", "*.zip") else {
        return;
    };
    // Make sure everything current is on disk first.
    app::with(|s| crate::settings::save(&s.settings));
    crate::tabs::save_session_now();
    // Website logins (cookies): the engine keeps its cookie file locked, so they
    // are read through WebView2 and stored as cookies.json.
    get_all_cookies(move |cookies| write_backup(out, cookies));
}

fn write_backup(out: PathBuf, cookies: Option<String>) {
    let hwnd = app::hwnd();
    let stage = std::env::temp_dir().join(format!("suppa-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let _ = std::fs::create_dir_all(&stage);
    let data = util::data_dir();
    let mut files = 0;
    for f in ["settings.ini", "saved.json", "session.txt"] {
        if std::fs::copy(data.join(f), stage.join(f)).is_ok() {
            files += 1;
        }
    }
    for rel in PROFILE_FILES {
        let src = profile_dir().join(rel);
        let dest = stage.join("profile").join(rel);
        if let Some(p) = dest.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if std::fs::copy(&src, &dest).is_ok() {
            files += 1;
        }
    }
    files += copy_tree(scripts::dir(), &stage.join("scripts"));
    files += copy_tree(&util::sub_dir("extensions"), &stage.join("extensions"));
    if let Some(c) = cookies {
        if std::fs::write(stage.join(COOKIES), c).is_ok() {
            files += 1;
        }
    }
    let _ = std::fs::write(
        stage.join(MARKER),
        format!("Suppa Browser backup\ndata_dir={}\nfiles={files}\n", data.display()),
    );
    let _ = std::fs::remove_file(&out);
    let result = run_tar(&[
        "-a".as_ref(),
        "-c".as_ref(),
        "-f".as_ref(),
        out.as_os_str(),
        "-C".as_ref(),
        stage.as_os_str(),
        ".".as_ref(),
    ]);
    let _ = std::fs::remove_dir_all(&stage);
    match result {
        Ok(()) => {
            util::log(&format!("backup saved: {} ({files} files)", out.display()));
            show_notice(&format!("✓  Backup saved ({files} files)"), &out.display().to_string());
        }
        Err(e) => {
            util::info(hwnd, &format!("Could not save the backup\n\n{e}"));
        }
    }
}

/// ≡ -> Restore: pick a backup .zip; it is applied when Suppa restarts.
pub fn restore() {
    let hwnd = app::hwnd();
    let Some(zip) = util::open_dialog("Restore Suppa backup", "Zip archive", "*.zip") else { return };
    let dir = restore_dir();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = run_tar(&["-x".as_ref(), "-f".as_ref(), zip.as_os_str(), "-C".as_ref(), dir.as_os_str()]) {
        let _ = std::fs::remove_dir_all(&dir);
        util::info(hwnd, &format!("Could not open the backup\n\n{e}"));
        return;
    }
    if !dir.join(MARKER).exists() {
        let _ = std::fs::remove_dir_all(&dir);
        util::info(hwnd, "That file is not a Suppa backup");
        return;
    }
    // Cancel = the unpacked files stay unused in "restore" without a marker... so
    // remove the marker first and put it back only on Yes.
    let marker = std::fs::read(dir.join(MARKER)).unwrap_or_default();
    let _ = std::fs::remove_file(dir.join(MARKER));
    crate::popup::confirm(
        "Restore this backup?",
        "Your current settings, saved tabs, open tabs, passwords and scripts are replaced. Suppa restarts to do this.",
        "Restore and restart",
        "Cancel",
        move || {
            if std::fs::write(restore_dir().join(MARKER), marker).is_ok() {
                util::log("backup restore scheduled; restarting");
                crate::window::restart_now();
            }
        },
    );
}

/// At startup, before the engine opens its files: apply a pending restore.
pub fn apply_pending() {
    let dir = restore_dir();
    if !dir.join(MARKER).exists() {
        return;
    }
    let data = util::data_dir();
    // Extensions were registered with their full folder path; fix it if the
    // backup came from another data folder (e.g. another Windows user name).
    let old_data = std::fs::read_to_string(dir.join(MARKER))
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("data_dir=").map(str::to_string)))
        .unwrap_or_default();
    let mut n = 0;
    for f in ["settings.ini", "saved.json", "session.txt"] {
        let src = dir.join(f);
        if !src.exists() {
            continue;
        }
        if f == "settings.ini" && !old_data.is_empty() {
            if let Ok(text) = std::fs::read_to_string(&src) {
                let fixed = text.replace(&old_data, &data.display().to_string());
                if std::fs::write(data.join(f), fixed).is_ok() {
                    n += 1;
                }
                continue;
            }
        }
        if std::fs::copy(&src, data.join(f)).is_ok() {
            n += 1;
        }
    }
    for rel in PROFILE_FILES {
        let src = dir.join("profile").join(rel);
        if src.exists() {
            let dest = profile_dir().join(rel);
            if let Some(p) = dest.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            // Old journal files belong to the replaced database.
            let _ = std::fs::remove_file(dest.with_file_name(format!(
                "{}-journal",
                dest.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
            )));
            if copy_retry(&src, &dest) {
                n += 1;
            }
        }
    }
    n += copy_tree(&dir.join("scripts"), scripts::dir());
    n += copy_tree(&dir.join("extensions"), &util::sub_dir("extensions"));
    // Cookies go back through WebView2 once the first page exists.
    if std::fs::copy(dir.join(COOKIES), data.join("cookies-import.json")).is_ok() {
        n += 1;
    }
    let _ = std::fs::remove_dir_all(&dir);
    util::log(&format!("backup restored ({n} files)"));
}

/// The engine of the previous Suppa may still hold a file for a moment.
fn copy_retry(src: &Path, dest: &Path) -> bool {
    for _ in 0..25 {
        if std::fs::copy(src, dest).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    util::log(&format!("restore: could not replace {}", dest.display()));
    false
}

pub fn show_notice(title: &str, detail: &str) {
    use crate::popup::{self, Item};
    popup::show(
        "notice",
        vec![Item::new(0, title).enabled(false), Item::new(0, detail).enabled(false), Item::sep(), Item::new(1, "OK")],
        crate::ui::corner_anchor(),
        true,
        |_| {},
    );
}

