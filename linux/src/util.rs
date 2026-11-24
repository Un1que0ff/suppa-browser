// Small helpers: data folders, URL handling, downloads, log.
use std::path::{Path, PathBuf};

/// ~/.local/share/SuppaBrowser (or $XDG_DATA_HOME/SuppaBrowser), created on demand.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
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

/// ~/Downloads (or the XDG download folder).
pub fn downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    // xdg-user-dirs: XDG_DOWNLOAD_DIR="$HOME/Downloads"
    if let Ok(text) = std::fs::read_to_string(home.join(".config").join("user-dirs.dirs")) {
        for l in text.lines() {
            if let Some(v) = l.strip_prefix("XDG_DOWNLOAD_DIR=") {
                let v = v.trim_matches('"').replace("$HOME", &home.to_string_lossy());
                let p = PathBuf::from(v);
                if p.is_dir() {
                    return p;
                }
            }
        }
    }
    let d = home.join("Downloads");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Opens a folder or file with the desktop's default program.
pub fn open_path(path: &Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
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
    let host = if host.starts_with('[') { host } else { host.split(':').next().unwrap_or(host) };
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
/// gets https://, files on disk become file://, anything else is a search.
pub fn normalize_input(input: &str, search_base: &str) -> String {
    let t = input.trim();
    if t.is_empty() {
        return String::new();
    }
    let lower = t.to_ascii_lowercase();
    // Suppa's own pages (the Windows names work too).
    for page in ["settings", "newtab"] {
        if lower == format!("suppa://{page}") || lower.starts_with(&format!("suppa://{page}/")) || lower.starts_with(&format!("https://suppa.{page}/")) {
            return format!("suppa://{page}/");
        }
    }
    if t.starts_with('/') || t.starts_with("~/") {
        let p = if let Some(rest) = t.strip_prefix("~/") {
            std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest)
        } else {
            PathBuf::from(t)
        };
        if p.exists() {
            return format!("file://{}", p.display().to_string().replace(' ', "%20"));
        }
    }
    let has_scheme = ["http://", "https://", "file://", "about:", "view-source:", "data:", "suppa://", "webkit://"]
        .iter()
        .any(|p| lower.starts_with(p));
    if has_scheme {
        return t.to_string();
    }
    if lower.starts_with("localhost") || lower.starts_with("127.0.0.1") {
        return format!("http://{t}");
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

/// Blocking HTTP(S) download with curl (part of every desktop Linux).
/// Call from a background thread only.
pub fn download(url: &str, dest: &Path) -> Result<(), String> {
    let tmp = dest.with_extension("part");
    let _ = std::fs::remove_file(&tmp);
    let ok = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "120", "-o"])
        .arg(&tmp)
        .arg(url)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let len = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if !ok || len == 0 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("download failed: {url}"));
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

/// Appends a line to <data>/suppa.log (for troubleshooting).
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

static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn mark_start() {
    let _ = START.get_or_init(std::time::Instant::now);
}

pub fn since_start_ms() -> u128 {
    START.get_or_init(std::time::Instant::now).elapsed().as_millis()
}
