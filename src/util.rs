// Small helpers: wide strings, data folders, URL handling, downloads.
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{MESSAGEBOX_STYLE, MessageBoxW, PostMessageW, SW_SHOWNORMAL};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

pub fn take_pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    webview2_com::take_pwstr(p)
}

/// %LOCALAPPDATA%\SuppaBrowser (created on demand).
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("SuppaBrowser");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn sub_dir(name: &str) -> PathBuf {
    let d = data_dir().join(name);
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn message_box(hwnd: HWND, text: &str, style: MESSAGEBOX_STYLE) -> i32 {
    unsafe { MessageBoxW(Some(hwnd), &HSTRING::from(text), w!("Suppa"), style).0 }
}

/// A message in the browser's own themed dialog (first line = title).
/// (`message_box` is kept only for when the browser engine itself failed.)
pub fn info(_hwnd: HWND, text: &str) {
    crate::popup::message(text);
}

pub fn post(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), msg, WPARAM(wparam), LPARAM(lparam));
    }
}

pub fn open_in_explorer(path: &Path) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(path.as_os_str()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Host part of a URL ("https://www.x.com/a" -> "www.x.com").
pub fn host_of(url: &str) -> String {
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => return String::new(),
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = if host.starts_with('[') {
        host
    } else {
        host.split(':').next().unwrap_or(host)
    };
    host.to_ascii_lowercase()
}

pub fn is_web_url(url: &str) -> bool {
    let l = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("ws://") || l.starts_with("wss://")
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Turns address-bar text into a URL: real URLs pass through, "example.com"
/// gets https://, anything else becomes a search.
/// Windows "Save as" dialog. Returns the chosen path.
pub fn save_dialog(title: &str, default_name: &str, type_name: &str, pattern: &str) -> Option<std::path::PathBuf> {
    file_dialog(true, title, default_name, type_name, pattern)
}

/// Windows "Open" dialog. Returns the chosen file.
pub fn open_dialog(title: &str, type_name: &str, pattern: &str) -> Option<std::path::PathBuf> {
    file_dialog(false, title, "", type_name, pattern)
}

fn file_dialog(save: bool, title: &str, default_name: &str, type_name: &str, pattern: &str) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileOpenDialog, FileSaveDialog, IFileDialog, SIGDN_FILESYSPATH};
    use windows::core::{HSTRING, Interface, PCWSTR};
    unsafe {
        let dlg: IFileDialog = if save {
            CoCreateInstance::<_, windows::Win32::UI::Shell::IFileSaveDialog>(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?.cast().ok()?
        } else {
            CoCreateInstance::<_, windows::Win32::UI::Shell::IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?.cast().ok()?
        };
        let (n, p) = (HSTRING::from(type_name), HSTRING::from(pattern));
        let spec = [COMDLG_FILTERSPEC { pszName: PCWSTR(n.as_ptr()), pszSpec: PCWSTR(p.as_ptr()) }];
        let _ = dlg.SetFileTypes(&spec);
        let _ = dlg.SetTitle(&HSTRING::from(title));
        if !default_name.is_empty() {
            let _ = dlg.SetFileName(&HSTRING::from(default_name));
        }
        if let Some(ext) = pattern.strip_prefix("*.") {
            let _ = dlg.SetDefaultExtension(&HSTRING::from(ext));
        }
        dlg.Show(Some(crate::app::hwnd())).ok()?;
        let item = dlg.GetResult().ok()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const _));
        s.map(std::path::PathBuf::from)
    }
}

/// Milliseconds since Suppa started (for the startup timing lines in the log).
pub fn since_start_ms() -> u128 {
    START.get_or_init(std::time::Instant::now).elapsed().as_millis()
}
static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Call first thing in main().
pub fn mark_start() {
    let _ = START.get_or_init(std::time::Instant::now);
}

pub fn normalize_input(input: &str, search_base: &str) -> String {
    let t = input.trim();
    if t.is_empty() {
        return String::new();
    }
    // A file on disk (e.g. an .html file opened from Explorer).
    if t.len() > 2 && t.as_bytes()[1] == b':' && std::path::Path::new(t.trim_matches('"')).exists() {
        return format!("file:///{}", t.trim_matches('"').replace('\\', "/").replace(' ', "%20"));
    }
    let lower = t.to_ascii_lowercase();
    let has_scheme = ["http://", "https://", "file://", "about:", "edge://", "chrome://", "chrome-extension://", "view-source:", "data:"]
        .iter()
        .any(|p| lower.starts_with(p));
    if has_scheme {
        return t.to_string();
    }
    if lower.starts_with("localhost") || lower.starts_with("127.0.0.1") {
        return format!("http://{t}");
    }
    // Windows paths
    if t.len() > 2 && t.as_bytes()[1] == b':' && (t.as_bytes()[2] == b'\\' || t.as_bytes()[2] == b'/') {
        return format!("file:///{}", t.replace('\\', "/"));
    }
    let first = t.split(['/', '?', '#']).next().unwrap_or("");
    let looks_like_host = !t.contains(' ')
        && first.contains('.')
        && !first.starts_with('.')
        && !first.ends_with('.')
        && first.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == ':');
    if looks_like_host {
        return format!("https://{t}");
    }
    format!("{search_base}?q={}", url_encode(t))
}

/// JS string literal (JSON-escaped).
pub fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Blocking HTTP(S) download using Windows' built-in urlmon (no extra
/// dependencies). Call from a background thread only.
pub fn download(url: &str, dest: &Path) -> Result<(), String> {
    use windows::Win32::System::Com::Urlmon::URLDownloadToFileW;
    let tmp = dest.with_extension("part");
    let _ = std::fs::remove_file(&tmp);
    unsafe {
        URLDownloadToFileW(None, &HSTRING::from(url), &HSTRING::from(tmp.as_os_str()), 0, None)
            .map_err(|e| format!("download failed: {url}: {e}"))?;
    }
    let len = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("download returned no data: {url}"));
    }
    let _ = std::fs::remove_file(dest);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

/// Appends a line to %LOCALAPPDATA%\SuppaBrowser\suppa.log (for troubleshooting).
pub fn log(line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(data_dir().join("suppa.log")) {
        let _ = writeln!(f, "[{}] {line}", unix_now());
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn file_age_secs(p: &Path) -> Option<u64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(std::time::SystemTime::now().duration_since(m).ok()?.as_secs())
}

/// Puts text on the Windows clipboard.
pub fn set_clipboard(text: &str) -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if OpenClipboard(Some(crate::app::hwnd())).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = (|| {
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let p = GlobalLock(mem) as *mut u16;
            if p.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(mem);
            SetClipboardData(13 /* CF_UNICODETEXT */, Some(HANDLE(mem.0))).ok()
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}