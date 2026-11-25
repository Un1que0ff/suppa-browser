// Downloads: Suppa's own list instead of the engine's download box.
//   * a popup next to the ≡ button opens when a download starts (and from the
//     download button that appears there), with progress, Open, Show in folder,
//     Pause / Resume, Cancel
//   * the Downloads page (suppa.downloads, Ctrl+J) lists every download
// The list is kept in downloads.json (downloads from incognito tabs are not).
use std::cell::RefCell;
use std::time::Instant;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows::core::BOOL;
use windows::core::HSTRING;

use crate::app::{self, *};
use crate::popup::{self, Item};
use crate::util;
use crate::webview::get_str;

const FILE: &str = "downloads.json";
const MAX: usize = 300;

struct Dl {
    id: u64,
    url: String,
    path: String,
    total: i64,
    got: i64,
    /// "active", "paused", "done", "cancelled", "failed"
    state: String,
    t: i64,
    incognito: bool,
    op: Option<ICoreWebView2DownloadOperation>,
    /// Polls without the file growing (finishes downloads of unknown size).
    stalled: u32,
}

#[derive(Default)]
struct Store {
    list: Vec<Dl>,
    loaded: bool,
    next_id: u64,
    popup_open: bool,
    last_notice: Option<Instant>,
}

thread_local! {
    static S: RefCell<Store> = RefCell::new(Store::default());
}

fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    S.with(|s| {
        let mut s = s.borrow_mut();
        if !s.loaded {
            s.loaded = true;
            load(&mut s);
        }
        f(&mut s)
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn load(s: &mut Store) {
    let Ok(text) = std::fs::read_to_string(util::data_dir().join(FILE)) else { return };
    let Ok(v) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else { return };
    for x in v {
        let st = x["state"].as_str().unwrap_or("done");
        s.next_id += 1;
        s.list.push(Dl {
            id: s.next_id,
            url: x["url"].as_str().unwrap_or("").into(),
            path: x["path"].as_str().unwrap_or("").into(),
            total: x["total"].as_i64().unwrap_or(0),
            got: x["got"].as_i64().unwrap_or(0),
            // Downloads that were still running when Suppa closed did not finish.
            state: if st == "active" || st == "paused" { "failed".into() } else { st.into() },
            t: x["t"].as_i64().unwrap_or(0),
            incognito: false,
            op: None,
            stalled: 0,
        });
    }
}

fn save(s: &Store) {
    let v: Vec<_> = s
        .list
        .iter()
        .filter(|d| !d.incognito)
        .map(|d| serde_json::json!({ "url": d.url, "path": d.path, "total": d.total, "got": d.got, "state": d.state, "t": d.t }))
        .collect();
    let _ = std::fs::write(util::data_dir().join(FILE), serde_json::Value::Array(v).to_string());
}

fn file_name(path: &str, url: &str) -> String {
    let p = path.rsplit(['\\', '/']).next().unwrap_or("");
    if !p.is_empty() {
        return p.to_string();
    }
    url.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("download").to_string()
}

fn size(b: i64) -> String {
    let b = b.max(0) as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{b} B")
    }
}

fn status(d: &Dl) -> String {
    match d.state.as_str() {
        "active" if d.total > 0 => format!("{} of {}  ({}%)", size(d.got), size(d.total), d.got * 100 / d.total.max(1)),
        "active" => size(d.got),
        "paused" => format!("Paused - {}", size(d.got)),
        "done" => {
            if std::path::Path::new(&d.path).exists() {
                size(d.total.max(d.got))
            } else {
                "Deleted".into()
            }
        }
        "cancelled" => "Cancelled".into(),
        _ => "Failed".into(),
    }
}

// ---- engine events ----------------------------------------------------------------------

/// A page started a download (webview.rs): it goes into our list, the engine's
/// own download box stays hidden.
pub fn on_starting(args: &ICoreWebView2DownloadStartingEventArgs, incognito: bool) {
    unsafe {
        let Ok(op) = args.DownloadOperation() else { return };
        let _ = args.SetHandled(true);
        let path = get_str(|p| args.ResultFilePath(p));
        let url = get_str(|p| op.Uri(p));
        let mut total = 0i64;
        let _ = op.TotalBytesToReceive(&mut total);
        let id = with(|s| {
            s.next_id += 1;
            let id = s.next_id;
            s.list.push(Dl { id, url, path, total, got: 0, state: "active".into(), t: now_ms(), incognito, op: Some(op.clone()), stalled: 0 });
            if s.list.len() > MAX {
                let extra = s.list.len() - MAX;
                s.list.drain(..extra);
            }
            save(s);
            id
        });
        let mut token = 0i64;
        let _ = op.add_BytesReceivedChanged(
            &BytesReceivedChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(op) = sender {
                    read_op(id, &op);
                }
                changed(false);
                Ok(())
            })),
            &mut token,
        );
        let _ = op.add_StateChanged(
            &StateChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(op) = sender {
                    read_op(id, &op);
                }
                with(|s| save(s));
                changed(true);
                Ok(())
            })),
            &mut token,
        );
        start_polling();        util::log("download started");
        changed(true);
        // Show the list next to the ≡ button (only if Suppa is in front).
        if !popup::is_open() && is_foreground() {
            crate::ui::post(serde_json::json!({ "type": "dl_open" }).to_string());
        }
    }
}

/// Reads a download's progress and state from the engine. False if the engine
/// no longer answers for it.
fn read_op(id: u64, op: &ICoreWebView2DownloadOperation) -> bool {
    unsafe {
        let mut st = COREWEBVIEW2_DOWNLOAD_STATE::default();
        if op.State(&mut st).is_err() {
            return false;
        }
        let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
        let _ = op.InterruptReason(&mut reason);
        let path = get_str(|p| op.ResultFilePath(p));
        let (mut got, mut total) = (0i64, 0i64);
        let _ = op.BytesReceived(&mut got);
        let _ = op.TotalBytesToReceive(&mut total);
        let mut can = BOOL(0);
        let _ = op.CanResume(&mut can);
        with(|s| {
            let Some(d) = s.list.iter_mut().find(|d| d.id == id) else { return };
            if !path.is_empty() {
                d.path = path;
            }
            d.got = got;
            if total > 0 {
                d.total = total;
            }
            d.state = if st == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                d.op = None;
                "done".into()
            } else if st == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
                if reason == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED {
                    d.op = None;
                    "cancelled".into()
                } else if can.as_bool() {
                    "paused".into()
                } else {
                    d.op = None;
                    "failed".into()
                }
            } else if d.state == "paused" {
                "paused".into()
            } else {
                "active".into()
            };
        });
        true
    }
}

// ---- polling while downloads run ---------------------------------------------------------
// A download started from a pop-up tab that closes itself (common for download
// links) stops sending events. So while anything runs, the list is also checked
// every 0.7 s: the engine is asked directly, and the file on disk tells how far
// it really is.

static POLLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn start_polling() {
    if POLLING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let hwnd = app::hwnd().0 as isize;
    std::thread::spawn(move || {
        while POLLING.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(700));
            util::post(windows::Win32::Foundation::HWND(hwnd as _), WM_APP_COMMAND, CMD_DL_POLL as usize, 0);
        }
    });
}

/// CMD_DL_POLL
pub fn poll() {
    let running: Vec<(u64, Option<ICoreWebView2DownloadOperation>, String, i64)> = with(|s| {
        s.list.iter().filter(|d| d.state == "active" || d.state == "paused").map(|d| (d.id, d.op.clone(), d.path.clone(), d.total)).collect()
    });
    if running.is_empty() {
        POLLING.store(false, std::sync::atomic::Ordering::SeqCst);
        return;
    }
    for (id, op, path, total) in running {
        if let Some(op) = &op {
            read_op(id, op);
        }
        // The file on disk (the engine writes straight to it, or to
        // "<file>.crdownload"): progress, and "done" once it is complete.
        let size = |p: &str| std::path::Path::new(p).metadata().map(|m| m.len() as i64).ok();
        let n = size(&format!("{path}.crdownload")).or_else(|| size(&path));
        with(|s| {
            let Some(d) = s.list.iter_mut().find(|d| d.id == id) else { return };
            if d.state != "active" {
                return;
            }
            let Some(n) = n else { return };
            if n > d.got {
                d.got = n;
                d.stalled = 0;
            } else {
                d.stalled += 1;
            }
            let complete = size(&path).is_some_and(|f| (total > 0 && f >= total) || (total <= 0 && d.stalled >= 6));
            if complete {
                d.state = "done".into();
                d.op = None;
            }
        });
    }    with(|s| save(s));
    notify();
}

fn is_foreground() -> bool {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() == app::hwnd() }
}

/// Something changed: bar button, open popup and Downloads pages follow.
/// Progress ticks are throttled; state changes go through at once.
fn changed(now: bool) {
    let due = with(|s| {
        let due = now || s.last_notice.is_none_or(|t| t.elapsed().as_millis() >= 400);
        if due {
            s.last_notice = Some(Instant::now());
        }
        due
    });
    if due {
        // Posted, so nothing runs inside the engine's event.
        util::post(app::hwnd(), WM_APP_COMMAND, CMD_DL_CHANGED as usize, 0);
    }
}

/// CMD_DL_CHANGED: tell everyone.
pub fn notify() {
    sync_bar();
    if with(|s| s.popup_open) && popup::is_open() {
        popup::refresh(popup_items());
    }
    crate::pages::broadcast("suppa.downloads", &list_json());
}

/// The download button next to ≡: shown once there is something this session.
pub fn sync_bar() {
    let show = crate::app::with(|s| s.settings.dl_button).unwrap_or(true);
    let (active, got, total) = with(|s| {
        let act: Vec<&Dl> = s.list.iter().filter(|d| d.state == "active").collect();
        (act.len(), act.iter().map(|d| d.got).sum::<i64>(), act.iter().map(|d| d.total).sum::<i64>())
    });
    let p = if total > 0 { got as f64 / total as f64 } else { -1.0 };
    crate::ui::post(serde_json::json!({ "type": "dl", "show": show, "active": active, "p": p }).to_string());
}

// ---- the popup next to ≡ --------------------------------------------------------------

const BASE: i64 = 800_000;
const ALL: i64 = BASE - 1;
const FOLDER: i64 = BASE - 2;
// per download: BASE + id*10 + action
const A_OPEN: i64 = 0;
const A_FOLDER: i64 = 1;
const A_PAUSE: i64 = 2;
const A_RESUME: i64 = 3;
const A_CANCEL: i64 = 4;
const A_REMOVE: i64 = 5;
const A_RETRY: i64 = 6;

fn popup_items() -> Vec<Item> {
    with(|s| {
        let mut items = Vec::new();
        let recent: Vec<&Dl> = s.list.iter().rev().take(8).collect();
        if recent.is_empty() {
            items.push(Item::new(0, "No downloads yet").enabled(false));
        }
        for d in recent {
            let a = |x: i64| BASE + d.id as i64 * 10 + x;
            let mut ctx = Vec::new();
            match d.state.as_str() {
                "active" => {
                    ctx.push(Item::new(a(A_PAUSE), "Pause"));
                    ctx.push(Item::new(a(A_CANCEL), "Cancel"));
                }
                "paused" => {
                    ctx.push(Item::new(a(A_RESUME), "Resume"));
                    ctx.push(Item::new(a(A_CANCEL), "Cancel"));
                }
                "done" => {
                    ctx.push(Item::new(a(A_OPEN), "Open"));
                    ctx.push(Item::new(a(A_FOLDER), "Show in folder"));
                }
                _ => ctx.push(Item::new(a(A_RETRY), "Download again")),
            }
            ctx.push(Item::sep());
            ctx.push(Item::new(a(A_REMOVE), "Remove from list"));
            // Click: open a finished file, pause / resume a running one. Right-click: all actions.
            let click = match d.state.as_str() {
                "done" => a(A_OPEN),
                "active" => a(A_PAUSE),
                "paused" => a(A_RESUME),
                _ => a(A_RETRY),
            };
            items.push(Item::new(click, file_name(&d.path, &d.url)).key(&status(d)).checked(d.state == "done").ctx(ctx));
        }
        items.push(Item::sep());
        items.push(Item::new(ALL, "Show all downloads").key("Ctrl+J"));
        items.push(Item::new(FOLDER, "Open downloads folder"));
        items
    })
}

/// The download button or a new download: the popup next to ≡.
pub fn show_popup() {
    with(|s| s.popup_open = true);
    popup::show("downloads", popup_items(), crate::ui::menu_anchor(), true, |pick| {
        with(|s| s.popup_open = false);
        match pick {
            Some(ALL) => open_page(),
            Some(FOLDER) => open_folder(),
            Some(id) if id >= BASE => act((id - BASE) as u64 / 10, (id - BASE) % 10),
            _ => {}
        }
    });
}

pub fn open_page() {
    crate::tabs::open_internal("https://suppa.downloads/");
}

fn open_folder() {
    let dir = with(|s| s.list.iter().rev().find(|d| !d.path.is_empty()).map(|d| d.path.clone()))
        .and_then(|p| std::path::Path::new(&p).parent().map(|x| x.to_path_buf()))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads"));
    util::open_in_explorer(&dir);
}

/// One action on one download.
fn act(id: u64, action: i64) {
    let Some((path, url, op)) = with(|s| s.list.iter().find(|d| d.id == id).map(|d| (d.path.clone(), d.url.clone(), d.op.clone()))) else {
        return;
    };
    unsafe {
        match action {
            A_OPEN => util::open_in_explorer(std::path::Path::new(&path)),
            A_FOLDER => util::show_in_folder(&path),
            A_PAUSE => {
                if let Some(op) = op {
                    let _ = op.Pause();
                    set_state(id, "paused");
                }
            }
            A_RESUME => {
                if let Some(op) = op {
                    let _ = op.Resume();
                    set_state(id, "active");
                }
            }
            A_CANCEL => {
                if let Some(op) = op {
                    let _ = op.Cancel();
                }
            }
            A_RETRY => crate::tabs::with_active_webview(|w| {
                let _ = w.Navigate(&HSTRING::from(url.as_str()));
            }),
            A_REMOVE => with(|s| {
                s.list.retain(|d| d.id != id || d.op.is_some() && d.state == "active");
                save(s);
            }),
            _ => {}
        }
    }
    notify();
}

fn set_state(id: u64, st: &str) {
    with(|s| {
        if let Some(d) = s.list.iter_mut().find(|d| d.id == id) {
            d.state = st.into();
        }
        save(s);
    });
}

// ---- the Downloads page -------------------------------------------------------------------

pub fn list_json() -> serde_json::Value {
    with(|s| {
        let items: Vec<_> = s
            .list
            .iter()
            .rev()
            .map(|d| {
                serde_json::json!({
                    "id": d.id, "name": file_name(&d.path, &d.url), "url": d.url, "path": d.path,
                    "state": d.state, "status": status(d), "t": d.t, "got": d.got, "total": d.total,
                    "exists": d.state == "done" && std::path::Path::new(&d.path).exists(),
                })
            })
            .collect();
        serde_json::json!({ "type": "dl_list", "items": items })
    })
}

/// {cmd:"dl", op, id} from the Downloads page.
pub fn on_page(op: &str, id: u64) {
    match op {
        "open" => act(id, A_OPEN),
        "folder" => act(id, A_FOLDER),
        "pause" => act(id, A_PAUSE),
        "resume" => act(id, A_RESUME),
        "cancel" => act(id, A_CANCEL),
        "retry" => act(id, A_RETRY),
        "remove" => act(id, A_REMOVE),
        "open_folder" => open_folder(),
        "clear" => {
            with(|s| {
                s.list.retain(|d| d.state == "active" || d.state == "paused");
                save(s);
            });
            notify();
        }
        _ => {}
    }
}
