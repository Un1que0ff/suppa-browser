// Save video / Save as MP3 (page right-click menu on a video or audio, the
// player popup, ≡ -> This page).
//
//  * A plain video / audio file (a <video src="https://...mp4">) is downloaded
//    straight into the Downloads folder.
//  * Streaming sites (YouTube, Vimeo, SoundCloud, ... - their players play
//    pieces, not one file) and every MP3 go through yt-dlp + FFmpeg, two free
//    programs. Suppa asks once before downloading them (from GitHub, ~110 MB)
//    into its data folder (tools\), and keeps yt-dlp up to date.
// The files land in your Downloads folder; a notice tells when they are done.
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::app::{self, *};
use crate::util;

const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const FFMPEG_URL: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";
const NO_WINDOW: u32 = 0x0800_0000;

/// Results waiting to be shown on the UI thread: (title, text).
static DONE: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn tools() -> PathBuf {
    util::sub_dir("tools")
}
fn ytdlp() -> PathBuf {
    tools().join("yt-dlp.exe")
}
fn ffmpeg() -> PathBuf {
    tools().join("ffmpeg.exe")
}

fn downloads_dir() -> PathBuf {
    let d = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn report(title: &str, text: &str) {
    DONE.lock().unwrap_or_else(|e| e.into_inner()).push((title.to_string(), text.to_string()));
    util::post(app::hwnd(), WM_APP_COMMAND, CMD_MEDIA_SAVED as usize, 0);
}

/// CMD_MEDIA_SAVED: show what finished.
pub fn on_done() {
    let list = std::mem::take(&mut *DONE.lock().unwrap_or_else(|e| e.into_inner()));
    for (t, x) in list {
        crate::backup::show_notice(&t, &x);
    }
}

/// A direct media file (not a stream, not blob:).
fn is_direct(src: &str) -> bool {
    let path = src.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    src.starts_with("http") && [".mp4", ".webm", ".mkv", ".mov", ".m4v", ".mp3", ".m4a", ".ogg", ".opus", ".wav", ".flac", ".aac"].iter().any(|e| path.ends_with(e))
}

/// Pages where "Save video" makes sense even without right-clicking the video.
pub fn is_video_page(url: &str) -> bool {
    let host = util::host_of(url);
    let host = host.strip_prefix("www.").unwrap_or(&host).strip_prefix("m.").unwrap_or(&host).to_string();
    (host == "youtube.com" && (url.contains("/watch") || url.contains("/shorts/") || url.contains("/live/")))
        || host == "youtu.be"
        || host == "music.youtube.com" && url.contains("watch")
        || ["vimeo.com", "dailymotion.com", "soundcloud.com", "twitch.tv", "tiktok.com", "x.com", "twitter.com", "reddit.com", "bandcamp.com", "rumble.com", "bilibili.com", "facebook.com", "instagram.com"]
            .iter()
            .any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// Save the media of `page` (and the element's own address `src`, if known).
/// `audio` = convert to MP3.
pub fn save(page: String, src: String, audio: bool) {
    if !audio && is_direct(&src) {
        let name = src.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("video.mp4").to_string();
        let name = util::url_decode(&name);
        let dest = unique(downloads_dir().join(if name.is_empty() { "video.mp4".to_string() } else { name }));
        crate::backup::show_notice("Saving video...", &dest.display().to_string());
        std::thread::spawn(move || match util::download(&src, &dest) {
            Ok(()) => report("✓  Video saved", &dest.display().to_string()),
            Err(e) => report("The video was not saved", &e),
        });
        return;
    }
    // Streams: the page address (yt-dlp finds the video); a direct file: its address.
    let target = if is_direct(&src) { src } else { page };
    if !target.starts_with("http") {
        util::info(app::hwnd(), "This video cannot be saved\n\nIt has no web address Suppa can download from.");
        return;
    }
    with_tools(move || run_ytdlp(target, audio));
}

fn unique(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    for i in 2..1000 {
        let q = p.with_file_name(format!("{stem} ({i}){ext}"));
        if !q.exists() {
            return q;
        }
    }
    p
}

/// Runs `then` once yt-dlp and FFmpeg are there (asks once before getting them).
fn with_tools(then: impl FnOnce() + Send + 'static) {
    if ytdlp().exists() && ffmpeg().exists() {
        std::thread::spawn(then);
        return;
    }
    crate::popup::confirm(
        &crate::i18n::tr("Download the video tools?"),
        &crate::i18n::tr("To save videos from streaming sites and to make MP3s, Suppa uses two free programs, yt-dlp and FFmpeg (about 110 MB, from GitHub). They are kept in Suppa's data folder and only run when you save something."),
        &crate::i18n::tr("Download and save"),
        &crate::i18n::tr("Cancel"),
        move || {
            crate::backup::show_notice("Getting the video tools...", "This happens once and takes a minute. Your file is saved right after.");
            std::thread::spawn(move || match get_tools() {
                Ok(()) => then(),
                Err(e) => report("The video tools could not be downloaded", &e),
            });
        },
    );
}

fn get_tools() -> Result<(), String> {
    let dir = tools();
    if !ytdlp().exists() {
        util::download(YTDLP_URL, &dir.join("yt-dlp.part")).map_err(|e| format!("yt-dlp: {e}"))?;
        std::fs::rename(dir.join("yt-dlp.part"), ytdlp()).map_err(|e| e.to_string())?;
    }
    if !ffmpeg().exists() {
        let zip = dir.join("ffmpeg.zip");
        util::download(FFMPEG_URL, &zip).map_err(|e| format!("FFmpeg: {e}"))?;
        let out = dir.join("ffmpeg-unpack");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let tar = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32\\tar.exe");
        let st = std::process::Command::new(tar).arg("-xf").arg(&zip).arg("-C").arg(&out).creation_flags(NO_WINDOW).status().map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&zip);
        let found = find(&out, "ffmpeg.exe");
        let r = match (st.success(), found) {
            (true, Some(f)) => std::fs::rename(&f, ffmpeg()).map_err(|e| e.to_string()),
            _ => Err("FFmpeg could not be unpacked".to_string()),
        };
        let _ = std::fs::remove_dir_all(&out);
        r?;
    }
    Ok(())
}

fn find(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            return Some(p);
        }
    }
    None
}

/// Background thread: yt-dlp saves the video / MP3 into Downloads.
fn run_ytdlp(url: String, audio: bool) {
    // Sites change often; yt-dlp updates itself at most once a week.
    if util::file_age_secs(&ytdlp()).is_some_and(|s| s > 7 * 86400) {
        let _ = std::process::Command::new(ytdlp()).arg("-U").creation_flags(NO_WINDOW).output();
        let _ = filetime_touch(&ytdlp());
    }
    let mut cmd = std::process::Command::new(ytdlp());
    cmd.arg("--no-playlist")
        .arg("--no-progress")
        .arg("--ffmpeg-location")
        .arg(tools())
        .arg("-P")
        .arg(downloads_dir())
        .arg("-o")
        .arg("%(title).150B [%(id)s].%(ext)s")
        .arg("--print")
        .arg("after_move:filepath");
    if audio {
        cmd.args(["-x", "--audio-format", "mp3", "--audio-quality", "0", "--embed-thumbnail", "--add-metadata"]);
    } else {
        cmd.args(["-f", "bv*[ext=mp4]+ba[ext=m4a]/bv*+ba/b", "--merge-output-format", "mp4"]);
    }
    cmd.arg("--").arg(&url).creation_flags(NO_WINDOW);
    util::log(&format!("save media: {} ({})", url, if audio { "mp3" } else { "video" }));
    match cmd.output() {
        Ok(o) if o.status.success() => {
            let path = String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("").trim().to_string();
            report(if audio { "✓  MP3 saved" } else { "✓  Video saved" }, if path.is_empty() { "In your Downloads folder" } else { &path });
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let line = err.lines().filter(|l| l.contains("ERROR")).last().unwrap_or("yt-dlp could not save it").to_string();
            util::log(&format!("save media failed: {err}"));
            report("It was not saved", line.trim_start_matches("ERROR: "));
        }
        Err(e) => report("It was not saved", &format!("yt-dlp did not start: {e}")),
    }
}

/// Marks yt-dlp as just checked for updates (its modified time = now).
fn filetime_touch(p: &std::path::Path) -> std::io::Result<()> {
    let f = std::fs::OpenOptions::new().append(true).open(p)?;
    f.set_modified(std::time::SystemTime::now())
}

/// The active tab's media (≡ -> This page, player popup).
pub fn save_tab(id: Option<u32>, audio: bool) {
    let url = app::with(|s| {
        let t = match id {
            Some(id) => s.tab(id),
            None => s.active_tab(),
        };
        t.map(|t| t.url.clone())
    })
    .flatten()
    .unwrap_or_default();
    save(url, String::new(), audio);
}
