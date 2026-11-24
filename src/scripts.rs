// The "scripts" folder next to Suppa.exe (like "py scripts" in Aviatema Tools).
// Holds the editable interface (ui.html/css/js), new-tab page, user scripts and
// custom filter rules. Everything is read at startup; the Update button restarts
// Suppa so changes take effect.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::util;

pub const UI_URL: &str = "https://suppa.ui/ui.html";

/// Default files, built into the exe so a missing folder is recreated.
const DEFAULTS: [(&str, &str); 11] = [
    ("webstore.js", include_str!("../scripts/webstore.js")),
    ("menu.html", include_str!("../scripts/menu.html")),
    ("ui.html", include_str!("../scripts/ui.html")),
    ("ui.css", include_str!("../scripts/ui.css")),
    ("ui.js", include_str!("../scripts/ui.js")),
    ("settings.html", include_str!("../scripts/settings.html")),
    ("cookies.js", include_str!("../scripts/cookies.js")),
    ("newtab.html", include_str!("../scripts/newtab.html")),
    ("README.txt", include_str!("../scripts/README.txt")),
    ("filters/my-filters.txt", include_str!("../scripts/filters/my-filters.txt")),
    ("userscripts/_example.js", include_str!("../scripts/userscripts/_example.js")),
];

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

pub fn exe_dir() -> PathBuf {
    exe_path().parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The scripts folder: <exe dir>\scripts. During development (exe inside
/// target\release) the project's own scripts folder is used instead.
pub fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let exe = exe_dir();
        let beside = exe.join("scripts");
        if beside.join("ui.html").exists() {
            return beside;
        }
        let mut p = exe.as_path();
        for _ in 0..3 {
            match p.parent() {
                Some(parent) => {
                    if parent.join("scripts").join("ui.html").exists() && parent.join("Cargo.toml").exists() {
                        return parent.join("scripts");
                    }
                    p = parent;
                }
                None => break,
            }
        }
        beside
    })
}

/// Recreates any default file that is missing (never overwrites your edits).
pub fn ensure_defaults() {
    let d = dir();
    for (name, content) in DEFAULTS {
        let p = d.join(name);
        if !p.exists() {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&p, content);
        }
    }
}

fn default_of(name: &str) -> Option<&'static str> {
    DEFAULTS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Reads a text file from the scripts folder (falls back to the built-in copy).
pub fn read_text(name: &str) -> String {
    std::fs::read_to_string(dir().join(name))
        .ok()
        .or_else(|| default_of(name).map(str::to_string))
        .unwrap_or_default()
}

fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

/// Serves https://suppa.ui/<path> from the scripts folder. Returns (mime, bytes).
pub fn serve(url: &str) -> Option<(&'static str, Vec<u8>)> {
    let rest = url.split_once("://")?.1;
    let path = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    let path = if path.is_empty() { "ui.html" } else { path };
    // No escaping the folder.
    if path.split('/').any(|seg| seg == ".." || seg.contains('\\') || seg.contains(':')) {
        return None;
    }
    let bytes = std::fs::read(dir().join(path))
        .ok()
        .or_else(|| default_of(path).map(|s| s.as_bytes().to_vec()))?;
    Some((mime_for(path), bytes))
}

/// User scripts (scripts\userscripts\*.js, not starting with "_"), each wrapped
/// so it only runs on its "// @match" patterns (all sites if none).
pub fn user_scripts() -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir().join("userscripts")) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.to_ascii_lowercase().ends_with(".js") && !name.starts_with('_')
        })
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|p| {
            let code = std::fs::read_to_string(p).ok()?;
            let matches: Vec<String> = code
                .lines()
                .filter_map(|l| l.trim().strip_prefix("//")?.trim().strip_prefix("@match").map(|m| m.trim().to_string()))
                .filter(|m| !m.is_empty())
                .collect();
            let name = p.file_name()?.to_string_lossy().to_string();
            let guard = if matches.is_empty() {
                "true".to_string()
            } else {
                format!(
                    "{}.some(function(p){{return new RegExp('^'+p.replace(/[.+?^${{}}()|[\\]\\\\]/g,'\\\\$&').replace(/\\*/g,'.*')+'$').test(location.href);}})",
                    serde_json::to_string(&matches).unwrap_or_else(|_| "[]".into())
                )
            };
            Some(format!(
                "(function(){{if(!({guard}))return;try{{\n{code}\n}}catch(e){{console.error('Suppa user script {name}:',e);}}}})();"
            ))
        })
        .collect()
}

/// Custom filter lists from scripts\filters\*.txt as (name, text).
pub fn custom_filters() -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(dir().join("filters")) else { return Vec::new() };
    let mut out: Vec<(String, String)> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("txt")).unwrap_or(false))
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            // Notepad may save with a BOM; it would corrupt the first rule.
            let text = text.trim_start_matches('\u{feff}').to_string();
            Some((p.file_name()?.to_string_lossy().to_string(), text))
        })
        .collect();
    out.sort();
    out
}

/// Cookie-banner clicker (scripts\cookies.js), read once per start.
pub fn cookie_script() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| read_text("cookies.js").trim_start_matches('\u{feff}').to_string())
}

fn walk(p: &Path, out: &mut Vec<(String, u64, u64)>, depth: u32) {
    let Ok(rd) = std::fs::read_dir(p) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() {
            if depth < 4 {
                walk(&path, out, depth + 1);
            }
            continue;
        }
        let t = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.push((path.to_string_lossy().to_string(), m.len(), t));
    }
}

// ---- Rust source (rebuilt by the Update button) ------------------------------------

/// The project folder with Cargo.toml + src\main.rs: the exe's folder or up to
/// 3 folders above it (dist\Suppa.exe -> the project folder).
pub fn source_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut p = Some(exe_dir());
        for _ in 0..4 {
            let d = p?;
            if d.join("Cargo.toml").exists() && d.join("src").join("main.rs").exists() {
                return Some(d);
            }
            p = d.parent().map(Path::to_path_buf);
        }
        None
    })
    .as_deref()
}

/// Files that make up the exe: src\, assets\, Cargo.toml, Cargo.lock, build.rs, app.manifest.
fn source_files() -> Vec<(String, u64, u64)> {
    let mut items = Vec::new();
    let Some(d) = source_dir() else { return items };
    walk(&d.join("src"), &mut items, 0);
    walk(&d.join("assets"), &mut items, 0);
    for f in ["Cargo.toml", "Cargo.lock", "build.rs", "app.manifest"] {
        if let Ok(m) = std::fs::metadata(d.join(f)) {
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64);
            items.push((f.to_string(), m.len(), t.unwrap_or(0)));
        }
    }
    items
}

fn exe_mtime() -> u64 {
    std::fs::metadata(exe_path())
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// True when some Rust source file is newer than the running exe.
pub fn rust_changed() -> bool {
    let exe = exe_mtime();
    exe > 0 && source_files().iter().any(|(_, _, t)| *t > exe + 1000)
}

fn cargo_exe() -> PathBuf {
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let p = PathBuf::from(home).join(".cargo").join("bin").join("cargo.exe");
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("cargo")
}

/// Builds the Rust code in the background (cargo build --release, no console
/// window), then puts the new exe in place of the running one (Windows allows
/// renaming a running exe). Posts WM_APP_BUILD_DONE with 1 on success.
pub fn start_build(hwnd: windows::Win32::Foundation::HWND) {
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
        let ok = build_and_swap();
        if let Err(e) = &ok {
            util::log(&format!("rebuild failed: {e}"));
        }
        util::post(hwnd, crate::app::WM_APP_BUILD_DONE, ok.is_ok() as usize, 0);
    });
}

pub fn build_log() -> PathBuf {
    util::data_dir().join("build.log")
}

fn build_and_swap() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let src = source_dir().ok_or("Rust source folder not found")?;
    let log = std::fs::File::create(build_log()).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    util::log(&format!("rebuilding from {}", src.display()));
    let exe = exe_path();
    let built = src.join("target").join("release").join("Suppa.exe");
    let old = exe.with_file_name("Suppa.old.exe");
    let _ = std::fs::remove_file(&old);
    // Running straight from target\release: move ourselves aside first so
    // cargo can write the new exe.
    let self_is_output = exe.canonicalize().ok() == built.canonicalize().ok();
    if self_is_output {
        std::fs::rename(&exe, &old).map_err(|e| format!("cannot move running exe: {e}"))?;
    }
    let status = std::process::Command::new(cargo_exe())
        .args(["build", "--release"])
        .env("CARGO_ENCODED_RUSTFLAGS", private_rustflags(src))
        .current_dir(src)
        .stdout(log)
        .stderr(log2)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        if self_is_output && !exe.exists() {
            let _ = std::fs::rename(&old, &exe);
        }
        return Err(format!("cargo build failed ({status})"));
    }
    if !self_is_output {
        std::fs::rename(&exe, &old).map_err(|e| format!("cannot move running exe: {e}"))?;
        if let Err(e) = std::fs::copy(&built, &exe) {
            let _ = std::fs::rename(&old, &exe);
            return Err(format!("cannot copy new exe: {e}"));
        }
    }
    util::log("rebuild ok");
    Ok(())
}

/// Watches the scripts folder, the Rust source and the exe folder with Windows
/// change notifications (no polling: zero CPU until a file changes). Posts
/// WM_APP_FILES_CHANGED to `hwnd` on any change.
pub fn start_watcher(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        FILE_NOTIFY_CHANGE, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FindFirstChangeNotificationW, FindNextChangeNotification,
    };
    use windows::Win32::System::Threading::{INFINITE, WaitForMultipleObjects};
    // (folder, include subfolders)
    let mut dirs: Vec<(PathBuf, bool)> = vec![(dir().to_path_buf(), true), (exe_dir(), false)];
    if let Some(src) = source_dir() {
        dirs.push((src.join("src"), true));
        dirs.push((src.join("assets"), true));
        dirs.push((src.to_path_buf(), false)); // Cargo.toml, build.rs, app.manifest
    }
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        let filter: FILE_NOTIFY_CHANGE =
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE;
        let handles: Vec<HANDLE> = dirs
            .iter()
            .filter(|(d, _)| d.is_dir())
            .filter_map(|(d, sub)| FindFirstChangeNotificationW(&windows::core::HSTRING::from(d.as_os_str()), *sub, filter).ok())
            .collect();
        if handles.is_empty() {
            return;
        }
        loop {
            let r = WaitForMultipleObjects(&handles, false, INFINITE);
            let i = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if i >= handles.len() {
                return;
            }
            util::post(windows::Win32::Foundation::HWND(hwnd_raw as *mut _), crate::app::WM_APP_FILES_CHANGED, 0, 0);
            if FindNextChangeNotification(handles[i]).is_err() {
                return;
            }
        }
    });
}

/// Compiler flags for rebuilding Suppa.exe (same as build.bat):
///  * static C runtime (runs without the Visual C++ Redistributable)
///  * your user folder and the project folder are written as "~" and "."
///    in the exe instead of their real paths (they would show your Windows
///    user name). This replaces .cargo\config.toml's flags while building.
fn private_rustflags(src: &Path) -> String {
    let mut flags = vec!["-Ctarget-feature=+crt-static".to_string()];
    if let Some(home) = std::env::var_os("USERPROFILE") {
        flags.push(format!("--remap-path-prefix={}=~", PathBuf::from(home).display()));
    }
    flags.push(format!("--remap-path-prefix={}=.", src.display()));
    flags.join("\u{1f}")
}

/// Removes Suppa.old.exe left behind by a rebuild.
pub fn remove_old_exe() {
    let _ = std::fs::remove_file(exe_path().with_file_name("Suppa.old.exe"));
}

/// Fingerprint of the scripts folder + Rust source + the exe (names, sizes,
/// modified times). Changes when anything is edited, added or removed.
pub fn signature() -> u64 {
    let mut items = Vec::new();
    walk(dir(), &mut items, 0);
    items.extend(source_files());
    if let Ok(m) = std::fs::metadata(exe_path()) {
        let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64);
        items.push(("exe".into(), m.len(), t.unwrap_or(0)));
    }
    items.sort();
    // FNV-1a
    let mut h: u64 = 0xcbf29ce484222325;
    for (name, len, t) in items {
        for b in name.bytes().chain(len.to_le_bytes()).chain(t.to_le_bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Starts a new copy of Suppa that waits for this one to exit, then loads
/// everything fresh (scripts folder + exe) and restores the tabs.
pub fn spawn_restart() -> Result<(), String> {
    let pid = std::process::id();
    std::process::Command::new(exe_path())
        .arg("--restart")
        .arg(pid.to_string())
        .current_dir(exe_dir())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// In the new copy: wait (max 15 s) until the old copy has fully exited.
pub fn wait_for_previous(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, 15_000);
            let _ = CloseHandle(h);
        }
    }
    // WebView2 needs a moment to release the profile folder.
    std::thread::sleep(std::time::Duration::from_millis(300));
    remove_old_exe();
    util::log(&format!("restarted (previous pid {pid})"));
}
