// The "scripts" folder next to the suppa binary: the same editable interface
// as the Windows version (ui.html/css/js, menus, settings page, cookie clicker,
// user scripts, own filter rules). The Update button restarts Suppa so changes
// take effect, and rebuilds it first when the Rust code changed.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::util;

pub const UI_URL: &str = "suppa://ui/ui.html";
pub const MENU_URL: &str = "suppa://ui/menu.html";

/// Default files, built into the binary so a missing folder is recreated.
const DEFAULTS: [(&str, &str); 11] = [
    ("webstore.js", include_str!("../../scripts/webstore.js")),
    ("menu.html", include_str!("../../scripts/menu.html")),
    ("ui.html", include_str!("../../scripts/ui.html")),
    ("ui.css", include_str!("../../scripts/ui.css")),
    ("ui.js", include_str!("../../scripts/ui.js")),
    ("settings.html", include_str!("../../scripts/settings.html")),
    ("cookies.js", include_str!("../../scripts/cookies.js")),
    ("newtab.html", include_str!("../../scripts/newtab.html")),
    ("README.txt", include_str!("../../scripts/README.txt")),
    ("filters/my-filters.txt", include_str!("../../scripts/filters/my-filters.txt")),
    ("userscripts/_example.js", include_str!("../../scripts/userscripts/_example.js")),
];

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

pub fn exe_dir() -> PathBuf {
    exe_path().parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The scripts folder: <binary dir>/scripts. During development (binary in a
/// cargo target folder) the project's own scripts folder is used.
pub fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let beside = exe_dir().join("scripts");
        // Development: SUPPA_SOURCE=<project>/linux uses the project's scripts folder.
        let dev = std::env::var_os("SUPPA_SOURCE").is_some();
        if beside.join("ui.html").exists() && !dev {
            return beside;
        }
        if let Some(src) = source_dir() {
            let p = src.parent().map(|p| p.join("scripts")).unwrap_or_default();
            if p.join("ui.html").exists() {
                return p;
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

pub fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
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

/// A file of the scripts folder by its path ("ui.html", "img/x.png"). No escaping the folder.
pub fn serve(path: &str) -> Option<(&'static str, Vec<u8>)> {
    let path = path.split(['?', '#']).next().unwrap_or("").trim_start_matches('/');
    if path.split('/').any(|seg| seg == ".." || seg.contains('\\')) {
        return None;
    }
    let bytes = std::fs::read(dir().join(path)).ok().or_else(|| default_of(path).map(|s| s.as_bytes().to_vec()))?;
    Some((mime_for(path), bytes))
}

/// User scripts (scripts/userscripts/*.js, not starting with "_"), each wrapped
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
            Some(format!("(function(){{if(!({guard}))return;try{{\n{code}\n}}catch(e){{console.error('Suppa user script {name}:',e);}}}})();"))
        })
        .collect()
}

/// Custom filter lists from scripts/filters/*.txt as (name, text).
pub fn custom_filters() -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(dir().join("filters")) else { return Vec::new() };
    let mut out: Vec<(String, String)> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("txt")).unwrap_or(false))
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            Some((p.file_name()?.to_string_lossy().to_string(), text.trim_start_matches('\u{feff}').to_string()))
        })
        .collect();
    out.sort();
    out
}

/// Cookie-banner clicker (scripts/cookies.js), read once per start.
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
        out.push((path.to_string_lossy().to_string(), m.len(), mtime_ms(&m)));
    }
}

fn mtime_ms(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---- Rust source (rebuilt by the Update button) ------------------------------------

/// The Linux project folder (linux/ with Cargo.toml + src/main.rs): the
/// binary's folder or up to 4 folders above it.
pub fn source_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        if let Some(d) = std::env::var_os("SUPPA_SOURCE").map(PathBuf::from) {
            if d.join("Cargo.toml").exists() {
                return Some(d);
            }
        }
        let mut p = Some(exe_dir());
        for _ in 0..5 {
            let d = p?;
            for cand in [d.clone(), d.join("linux")] {
                if cand.join("Cargo.toml").exists() && cand.join("src").join("main.rs").exists() {
                    if std::fs::read_to_string(cand.join("Cargo.toml")).map(|t| t.contains("suppa-linux")).unwrap_or(false) {
                        return Some(cand);
                    }
                }
            }
            p = d.parent().map(Path::to_path_buf);
        }
        None
    })
    .as_deref()
}

/// Files that make up the binary: linux/src, linux/Cargo.toml, the shared settings code.
fn source_files() -> Vec<(String, u64, u64)> {
    let mut items = Vec::new();
    let Some(d) = source_dir() else { return items };
    walk(&d.join("src"), &mut items, 0);
    for f in [d.join("Cargo.toml"), d.join("..").join("src").join("settings.rs")] {
        if let Ok(m) = std::fs::metadata(&f) {
            items.push((f.to_string_lossy().to_string(), m.len(), mtime_ms(&m)));
        }
    }
    items
}

fn exe_mtime() -> u64 {
    std::fs::metadata(exe_path()).map(|m| mtime_ms(&m)).unwrap_or(0)
}

/// True when some Rust source file is newer than the running binary.
pub fn rust_changed() -> bool {
    let exe = exe_mtime();
    exe > 0 && source_files().iter().any(|(_, _, t)| *t > exe + 1000)
}

pub fn build_log() -> PathBuf {
    util::data_dir().join("build.log")
}

fn cargo_exe() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".cargo").join("bin").join("cargo");
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("cargo")
}

/// cargo build --release, then the new binary replaces the running one
/// (Linux allows replacing a running file). Blocking: run on a thread.
pub fn build_and_swap() -> Result<(), String> {
    let src = source_dir().ok_or("Rust source folder not found")?;
    let log = std::fs::File::create(build_log()).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    util::log(&format!("rebuilding from {}", src.display()));
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| src.join("target"));
    let mut flags = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        flags.push(format!("--remap-path-prefix={}=~", PathBuf::from(home).display()));
    }
    if let Some(root) = src.parent() {
        flags.push(format!("--remap-path-prefix={}=.", root.display()));
    }
    let status = std::process::Command::new(cargo_exe())
        .args(["build", "--release"])
        .env("CARGO_ENCODED_RUSTFLAGS", flags.join("\u{1f}"))
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(src)
        .stdout(log)
        .stderr(log2)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build failed ({status})"));
    }
    let built = target.join("release").join("suppa");
    let exe = exe_path();
    if built.canonicalize().ok() != exe.canonicalize().ok() {
        // Write next to it, then rename over it (the running file stays valid).
        let tmp = exe.with_extension("new");
        std::fs::copy(&built, &tmp).map_err(|e| format!("cannot copy new binary: {e}"))?;
        std::fs::rename(&tmp, &exe).map_err(|e| format!("cannot replace binary: {e}"))?;
    }
    util::log("rebuild ok");
    Ok(())
}

/// Fingerprint of the scripts folder + Rust source + the binary.
pub fn signature() -> u64 {
    let mut items = Vec::new();
    walk(dir(), &mut items, 0);
    items.extend(source_files());
    if let Ok(m) = std::fs::metadata(exe_path()) {
        items.push(("exe".into(), m.len(), mtime_ms(&m)));
    }
    items.sort();
    let mut h: u64 = 0xcbf29ce484222325;
    for (name, len, t) in items {
        for b in name.bytes().chain(len.to_le_bytes()).chain(t.to_le_bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}
