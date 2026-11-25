// Tab management: open, close, switch, navigate, reopen.
// Tabs restored from the last session stay unloaded (no process, no RAM)
// until you click them.
use std::time::Instant;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
use windows::core::{HSTRING, Interface};

use crate::app::{self, Tab};
use crate::{newtab, ui, util, webview};

/// Built-in settings page (scripts\settings.html).
pub const SETTINGS_HOST: &str = "suppa.settings";
pub const SETTINGS_URL: &str = "https://suppa.settings/";

fn insert(url: String, at: Option<usize>, incognito: bool) -> Option<(u32, usize)> {
    app::with(|s| {
        let id = s.next_id;
        s.next_id += 1;
        let idx = at.unwrap_or(if s.tabs.is_empty() { 0 } else { s.active + 1 }).min(s.tabs.len());
        let mut tab = Tab::new(id, url);
        tab.incognito = incognito;
        s.tabs.insert(idx, tab);
        // Opened next to a grouped tab (link, popup): joins that group.
        if at.is_none() {
            crate::groups::inherit(s, idx);
        }
        if idx <= s.active && s.tabs.len() > 1 {
            s.active += 1;
        }
        ui::insert_tab_item(s, idx);
        (id, idx)
    })
}

fn home_url() -> String {
    app::with(|s| s.settings.home.clone()).unwrap_or_else(|| crate::settings::DEFAULT_HOME.to_string())
}

/// Opens a new tab. `url` None = home page, added at the end with the
/// cursor in the address bar (like Ctrl+T in Chrome).
pub fn new_tab(url: Option<String>, activate_it: bool) -> Option<u32> {
    open(url, activate_it, false)
}

/// Ctrl+Shift+N: a private tab (own profile, nothing kept after closing).
pub fn new_incognito() {
    open(None, true, true);
}

pub fn new_incognito_url(url: String) {
    open(Some(url), true, true);
}

/// Dragged in the tab row: moves tab `from` to position `to`.
pub fn move_tab(from: usize, to: usize) {
    app::with(|s| {
        if from >= s.tabs.len() || to >= s.tabs.len() || from == to {
            return;
        }
        let active_id = s.tabs.get(s.active).map(|t| t.id);
        let t = s.tabs.remove(from);
        s.tabs.insert(to, t);
        crate::groups::after_move(s, to);
        if let Some(id) = active_id {
            s.active = s.tabs.iter().position(|t| t.id == id).unwrap_or(0);
        }
        ui::select_tab_item(s, s.active);
    });
}

/// Dragged out of the window: the tab opens in a new Suppa window where it
/// was dropped (same logins and extensions), and leaves this one.
pub fn detach(idx: usize) {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowPlacement, GetWindowRect, WINDOWPLACEMENT};
    let Some((n, tab, hwnd)) = app::with(|s| (s.tabs.len(), s.tabs.get(idx).map(|t| (t.url.clone(), t.incognito)), s.hwnd)) else {
        return;
    };
    let Some((url, incognito)) = tab else { return };
    if n < 2 || url.is_empty() {
        return;
    }
    let (mut pt, mut wr) = (POINT::default(), RECT::default());
    let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    unsafe {
        let _ = GetCursorPos(&mut pt);
        let _ = GetWindowRect(hwnd, &mut wr);
        let _ = GetWindowPlacement(hwnd, &mut wp);
    }
    let inside = pt.x >= wr.left && pt.x < wr.right && pt.y >= wr.top && pt.y < wr.bottom;
    if inside {
        return;
    }
    // New window: same size as this one (its normal size if maximized), under the mouse.
    let r = wp.rcNormalPosition;
    let (w, h) = ((r.right - r.left).max(500), (r.bottom - r.top).max(400));
    let (x, y) = (pt.x - 120, pt.y - 18);
    let mut cmd = std::process::Command::new(crate::scripts::exe_path());
    cmd.arg("--window").arg(format!("{x},{y},{w},{h}"));
    if incognito {
        cmd.arg("--incognito");
    }
    cmd.arg(&url).current_dir(crate::scripts::exe_dir());
    match cmd.spawn() {
        Ok(_) => {
            util::log("tab moved to a new window");
            close_tab(idx);
        }
        Err(e) => util::log(&format!("new window failed: {e}")),
    }
}

pub fn reload_index(idx: usize) {
    if let Some(Some(wv)) = app::with(|s| s.tabs.get(idx).and_then(|t| t.webview.clone())) {
        unsafe {
            let _ = wv.Reload();
        }
    } else {
        activate(idx);
    }
}

pub fn duplicate_index(idx: usize) {
    if let Some(Some((url, inc))) = app::with(|s| s.tabs.get(idx).map(|t| (t.url.clone(), t.incognito))) {
        open(Some(url), true, inc);
    }
}

pub fn close_others_except(idx: usize) {
    let Some(Some(keep)) = app::with(|s| s.tabs.get(idx).map(|t| t.id)) else { return };
    loop {
        let other = app::with(|s| s.tabs.iter().position(|t| t.id != keep)).flatten();
        match other {
            Some(i) => close_tab(i),
            None => break,
        }
    }
}

fn open(url: Option<String>, activate_it: bool, incognito: bool) -> Option<u32> {
    let blank = url.is_none();
    let at = if blank { Some(usize::MAX) } else { None };
    let (id, idx) = insert(url.unwrap_or_else(home_url), at, incognito)?;
    if blank {
        app::with(|s| {
            if let Some(t) = s.tab_mut(id) {
                t.focus_address = true;
            }
        });
    }
    ui::layout();
    if activate_it {
        activate(idx);
        if blank {
            ui::focus_address();
        }
    } else {
        webview::create(id);
    }
    Some(id)
}

/// Opens the settings tab (or switches to it if it is already open).
pub fn open_settings() {
    let existing = app::with(|s| s.tabs.iter().position(|t| util::host_of(&t.url) == SETTINGS_HOST)).flatten();
    match existing {
        Some(i) => activate(i),
        None => {
            let _ = open(Some(SETTINGS_URL.to_string()), true, false);
        }
    }
}

/// Opens one of Suppa's own pages (History, Downloads), or switches to it.
pub fn open_internal(url: &str) {
    let host = util::host_of(url);
    let existing = app::with(|s| s.tabs.iter().position(|t| util::host_of(&t.url) == host)).flatten();
    match existing {
        Some(i) => activate(i),
        None => {
            let _ = open(Some(url.to_string()), true, false);
        }
    }
}

/// Adds a tab without loading it (session restore). It loads when activated.
pub fn add_lazy(url: String) {
    let _ = insert(url, Some(usize::MAX), false);
}

/// Opens a tab for a popup / target=_blank link. The page's request is held
/// (deferral) until the new WebView exists, so window.opener keeps working.
/// The new tab uses the same profile as the page that opened it.
pub fn new_tab_for_window(
    uri: String,
    args: ICoreWebView2NewWindowRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
    background: bool,
    incognito: bool,
) {
    let Some((id, idx)) = insert(uri, None, incognito) else {
        let _ = unsafe { deferral.Complete() };
        return;
    };
    app::with(|s| {
        if let Some(t) = s.tab_mut(id) {
            t.pending_new_window = Some((args, deferral));
        }
    });
    ui::layout();
    if background {
        webview::create(id);
    } else {
        activate(idx);
    }
}

/// Shows tab `idx`, hides the previous one and loads it if needed.
pub fn activate(idx: usize) {
    let r = app::with(|s| {
        if idx >= s.tabs.len() {
            return None;
        }
        let prev = if s.active != idx { s.tabs.get(s.active).and_then(|t| t.controller.clone()) } else { None };
        s.active = idx;
        // A tab in a collapsed group opens the group.
        let g = s.tabs[idx].group;
        if let Some(grp) = s.groups.iter_mut().find(|x| x.id == g) {
            grp.collapsed = false;
        }
        let minimized = s.minimized;
        let t = &mut s.tabs[idx];
        t.last_active = Instant::now();
        t.suspended = false;
        let ctrl = t.controller.clone();
        let id = t.id;
        let ntp = newtab::is_newtab(&t.url);
        ui::select_tab_item(s, idx);
        ui::update_tab_item(s, idx);
        ui::refresh_toolbar(s);
        Some((prev, ctrl, id, ui::content_rect(s), minimized, ntp))
    })
    .flatten();
    let Some((prev, ctrl, id, rect, minimized, ntp)) = r else { return };
    unsafe {
        if let Some(p) = prev {
            let _ = p.SetIsVisible(false);
            webview::set_memory_low(&p, true);
        }
        match ctrl {
            Some(c) => {
                let _ = c.SetBounds(rect);
                let _ = c.SetIsVisible(!minimized);
                webview::set_memory_low(&c, false);
                if !ntp {
                    let _ = c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
                }
            }
            None => webview::create(id),
        }
    }
}

pub fn activate_relative(delta: i32) {
    if let Some((n, a)) = app::with(|s| (s.tabs.len(), s.active)) {
        if n > 1 {
            let i = (a as i32 + delta).rem_euclid(n as i32) as usize;
            activate(i);
        }
    }
}

pub fn activate_number(n: usize) {
    // Ctrl+1..8 = that tab, Ctrl+9 = last tab (like Chrome).
    if let Some(len) = app::with(|s| s.tabs.len()) {
        if len == 0 {
            return;
        }
        let i = if n >= 8 { len - 1 } else { n.min(len - 1) };
        activate(i);
    }
}

pub fn close_tab(idx: usize) {
    let r = app::with(|s| {
        if idx >= s.tabs.len() {
            return None;
        }
        let t = s.tabs.remove(idx);
        crate::groups::cleanup(s);
        ui::remove_tab_item(s, idx);
        if !newtab::is_newtab(&t.url) && !t.incognito {
            s.closed.push((t.url.clone(), idx));
            if s.closed.len() > 25 {
                s.closed.remove(0);
            }
        }
        let was_active = idx == s.active;
        if s.tabs.is_empty() {
            return Some((t, None, true));
        }
        if idx < s.active || s.active >= s.tabs.len() {
            s.active = s.active.saturating_sub(1).min(s.tabs.len() - 1);
        }
        let next = if was_active { Some(idx.min(s.tabs.len() - 1)) } else { None };
        if was_active {
            s.active = next.unwrap();
        }
        Some((t, next, false))
    })
    .flatten();
    let Some((tab, next, last)) = r else { return };
    if let Some((_, d)) = &tab.pending_new_window {
        let _ = unsafe { d.Complete() };
    }
    if let Some(c) = tab.controller {
        let _ = unsafe { c.Close() };
    }
    if last {
        util::post(app::hwnd(), WM_CLOSE, 0, 0);
        return;
    }
    ui::layout();
    match next {
        Some(i) => activate(i),
        None => {
            app::with(|s| {
                ui::select_tab_item(s, s.active);
                ui::refresh_toolbar(s);
            });
        }
    }
}

pub fn close_tab_id(id: u32) {
    if let Some(Some(i)) = app::with(|s| s.tab_index(id)) {
        close_tab(i);
    }
}

pub fn close_active() {
    if let Some(i) = app::with(|s| s.active) {
        close_tab(i);
    }
}

pub fn close_others() {
    if let Some(a) = app::with(|s| s.active) {
        close_others_except(a);
    }
}

/// Ctrl+Shift+T: reopens the last closed tab at the place it was.
pub fn reopen_closed() {
    let Some(Some((url, at))) = app::with(|s| s.closed.pop()) else { return };
    if let Some((_, idx)) = insert(url, Some(at), false) {
        ui::layout();
        activate(idx);
    }
}

pub fn duplicate_active() {
    if let Some(Some((url, inc))) = app::with(|s| s.active_tab().map(|t| (t.url.clone(), t.incognito))) {
        open(Some(url), true, inc);
    }
}

/// Navigates the active tab to address-bar text (URL or search).
pub fn navigate_active(text: &str) {
    let r = app::with(|s| {
        let url = util::normalize_input(text, s.settings.search_base());
        if url.is_empty() {
            return None;
        }
        let t = s.tabs.get_mut(s.active)?;
        t.url = url.clone();
        Some((t.id, t.webview.clone(), url))
    })
    .flatten();
    let Some((id, wv, url)) = r else { return };
    match wv {
        Some(wv) => unsafe {
            let _ = wv.Navigate(&HSTRING::from(url));
        },
        None => webview::create(id),
    }
}

pub fn with_active_webview(f: impl FnOnce(&ICoreWebView2)) {
    if let Some(Some(wv)) = app::with(|s| s.active_tab().and_then(|t| t.webview.clone())) {
        f(&wv);
    }
}

pub fn go_back() {
    with_active_webview(|w| unsafe {
        let _ = w.GoBack();
    });
}
pub fn go_forward() {
    with_active_webview(|w| unsafe {
        let _ = w.GoForward();
    });
}
pub fn reload() {
    with_active_webview(|w| unsafe {
        let _ = w.Reload();
    });
}
pub fn hard_reload() {
    // Bypasses cache via DevTools protocol.
    with_active_webview(|w| unsafe {
        let _ = w.CallDevToolsProtocolMethod(
            windows::core::w!("Page.reload"),
            windows::core::w!("{\"ignoreCache\":true}"),
            None::<&ICoreWebView2CallDevToolsProtocolMethodCompletedHandler>,
        );
    });
}
pub fn go_home() {
    navigate_active(&home_url());
}

pub fn zoom(delta: f64) {
    if let Some(Some(c)) = app::with(|s| s.active_tab().and_then(|t| t.controller.clone())) {
        unsafe {
            let mut z = 1.0f64;
            let _ = c.ZoomFactor(&mut z);
            let nz = if delta == 0.0 { 1.0 } else { (z + delta).clamp(0.25, 5.0) };
            let _ = c.SetZoomFactor(nz);
        }
    }
}

pub fn print() {
    with_active_webview(|w| unsafe {
        if let Ok(w16) = w.cast::<ICoreWebView2_16>() {
            let _ = w16.ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_BROWSER);
        }
    });
}

pub fn devtools() {
    with_active_webview(|w| unsafe {
        let _ = w.OpenDevToolsWindow();
    });
}

pub fn task_manager() {
    with_active_webview(|w| unsafe {
        if let Ok(w6) = w.cast::<ICoreWebView2_6>() {
            let _ = w6.OpenTaskManagerWindow();
        }
    });
}

/// URLs of all tabs (for session saving) + tab-group lines.
pub fn session_urls() -> (Vec<String>, usize, Vec<String>) {
    app::with(|s| {
        let mut urls = Vec::new();
        let mut active = 0;
        let mut meta = crate::groups::session_meta_groups(s);
        for (i, t) in s.tabs.iter().enumerate() {
            if newtab::is_newtab(&t.url) || t.incognito {
                continue;
            }
            if i == s.active {
                active = urls.len();
            }
            if t.group != 0 {
                meta.push(format!("#tabgroup\t{}\t{}", urls.len(), t.group));
            }
            urls.push(t.url.clone());
        }
        (urls, active, meta)
    })
    .unwrap_or_default()
}

/// Saves the open tabs (and their groups) to session.txt.
pub fn save_session_now() {
    let (urls, active, meta) = session_urls();
    crate::settings::save_session(&urls, active, &meta);
}
