// Top bar host. The bar itself is HTML/CSS/JS from the scripts folder
// (scripts\ui.html), shown in a small WebView above the page. This file:
//   * creates that WebView and passes messages to/from it,
//   * lays out the bar and the page, paints the rounded frame around the page,
//   * applies dark/light mode to the title bar and menus.
use std::cell::Cell;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, Interface, PCSTR, PWSTR, w};

use crate::app::{self, *};
use crate::{newtab, scripts, settings, tabs, util};

/// Room the bar rows take at each window edge, in window px: (top, bottom, left, right).
/// Each row (tabs, address bar, saved tabs) can be at any edge (Settings -> Layout);
/// rows at the top/bottom are stacked, rows at the left/right share one column.
pub fn edges(s: &app::State) -> (i32, i32, i32, i32) {
    if s.fullscreen.is_some() {
        return (0, 0, 0, 0);
    }
    let st = &s.settings;
    let (mut t, mut b, mut l, mut r) = (0, 0, false, false);
    for (row, h) in [("tabs", st.row_tabs), ("nav", st.row_nav), ("saved", st.row_saved)] {
        if row == "saved" && !st.show_saved {
            continue;
        }
        match st.row_pos(row) {
            "bottom" => b += h,
            "left" => l = true,
            "right" => r = true,
            _ => t += h,
        }
    }
    let side = if l || r { scaled(st.side_width, s) } else { 0 };
    (scaled(t, s), scaled(b, s), if l { side } else { 0 }, if r { side } else { 0 })
}

/// No row at the top: the window buttons float over the page (see winbtn.rs).
pub fn floating_buttons(s: &app::State) -> bool {
    s.fullscreen.is_none() && edges(s).0 == 0
}

pub struct Ui {
    pub controller: Option<ICoreWebView2Controller>,
    pub webview: Option<ICoreWebView2>,
    pub ready: bool,
    pub dark: bool,
    pub bg: COLORREF,
    pub border: COLORREF,
    pub update_available: bool,
    /// Current accent colour as shown by the bar (menus use it too).
    pub accent: String,
}

impl Ui {
    pub fn new(dark: bool) -> Self {
        let (bg, border) = default_colors(dark);
        Ui { controller: None, webview: None, ready: false, dark, bg, border, update_available: false, accent: "#b07cff".into() }
    }
}

fn default_colors(dark: bool) -> (COLORREF, COLORREF) {
    if dark { (rgb(0x16, 0x16, 0x16), rgb(0x38, 0x38, 0x38)) } else { (rgb(0xf2, 0xf2, 0xf2), rgb(0xd0, 0xd0, 0xd0)) }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn parse_hex(s: &str) -> Option<COLORREF> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn sc(v: i32, dpi: u32) -> i32 {
    v * dpi as i32 / 96
}

/// Interface scale (Settings -> Interface scale) as a factor, e.g. 1.25.
pub fn zoom(s: &app::State) -> f64 {
    s.settings.ui_scale as f64 / 100.0
}

/// CSS px of the bar -> window px (DPI x interface scale).
pub fn scaled(css: i32, s: &app::State) -> i32 {
    (css as f64 * s.dpi as f64 / 96.0 * zoom(s)).round() as i32
}

/// Area where the active web page is shown (inside the rounded frame).
pub fn content_rect(s: &app::State) -> RECT {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(s.hwnd, &mut rc);
    }
    if s.fullscreen.is_some() {
        return rc;
    }
    // The page fills the window, except where the bar rows are.
    // A thin strip stays free at the top (no row on top) so the window can be resized there.
    let (t, b, l, r) = edges(s);
    let r = RECT { left: l, top: t.max(top_strip(s)), right: rc.right - r, bottom: rc.bottom - b };
    RECT { right: r.right.max(r.left), bottom: r.bottom.max(r.top), ..r }
}

/// No row at the top: height of the free strip at the window's top edge
/// (for resizing; 0 when maximized).
pub fn top_strip(s: &app::State) -> i32 {
    let max = unsafe { IsZoomed(s.hwnd).as_bool() };
    if edges(s).0 == 0 && s.fullscreen.is_none() && !max { sc(4, s.dpi) } else { 0 }
}

/// The bar WebView fills the whole window, behind the page (its rows sit
/// around the page; the hole in the middle is covered by the page).
fn bar_rect(s: &app::State) -> RECT {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(s.hwnd, &mut rc);
    }
    rc
}

/// Is a client point in the free top strip (bar at the bottom)? Used for resizing.
pub fn in_top_strip(y: i32) -> bool {
    app::with(|s| y >= 0 && y < top_strip(s)).unwrap_or(false)
}

// ---- Theme ---------------------------------------------------------------------------

type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type FlushMenuThemes = unsafe extern "system" fn();

/// Dark title bar + dark popup menus (Windows 10 1903+).
pub fn apply_window_theme(hwnd: HWND, dark: bool) {
    unsafe {
        let v = BOOL(dark as i32);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const _ as *const _, 4);
        // uxtheme ordinals 135/136 (undocumented but stable; used by many apps).
        if let Ok(ux) = LoadLibraryW(w!("uxtheme.dll")) {
            if let Some(f) = GetProcAddress(ux, PCSTR(135 as *const u8)) {
                let set: SetPreferredAppMode = std::mem::transmute(f);
                set(if dark { 2 } else { 3 }); // ForceDark / ForceLight
            }
            if let Some(f) = GetProcAddress(ux, PCSTR(136 as *const u8)) {
                let flush: FlushMenuThemes = std::mem::transmute(f);
                flush();
            }
        }
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

/// Paints the window background (seen only while the bar/page are loading).
pub fn paint(hwnd: HWND) {
    let bg = app::with(|s| s.ui.bg).unwrap_or(rgb(0x16, 0x16, 0x16));
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let brush = CreateSolidBrush(bg);
        FillRect(hdc, &rc, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = EndPaint(hwnd, &ps);
    }
}

// ---- Bar WebView ------------------------------------------------------------------------

thread_local! {
    /// Child window that holds the bar WebView. It covers the whole window and
    /// is kept at the bottom, so the page (a sibling) is always drawn on top.
    static HOST: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
}

unsafe extern "system" fn host_proc(h: HWND, msg: u32, wp: windows::Win32::Foundation::WPARAM, lp: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    unsafe {
        if msg == WM_PAINT {
            paint(h);
            return windows::Win32::Foundation::LRESULT(0);
        }
        if msg == WM_ERASEBKGND {
            return windows::Win32::Foundation::LRESULT(1);
        }
        DefWindowProcW(h, msg, wp, lp)
    }
}

fn create_host(parent: HWND) -> HWND {
    unsafe {
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(host_proc),
            hInstance: hinst.into(),
            lpszClassName: w!("SuppaBarHost"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let mut rc = RECT::default();
        let _ = GetClientRect(parent, &mut rc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("SuppaBarHost"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            0,
            0,
            rc.right,
            rc.bottom,
            Some(parent),
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap_or_default()
    }
}

/// Keeps the window stack right: the bar's window under every page, the
/// floating window buttons above them.
pub fn restack() {
    let host = HOST.with(|h| h.get());
    if !host.is_invalid() {
        unsafe {
            let _ = SetWindowPos(host, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
    crate::winbtn::layout();
}

/// Creates the bar WebView (called once the WebView2 environment exists).
pub fn create(env: &ICoreWebView2Environment, hwnd: HWND) {
    let host = create_host(hwnd);
    HOST.with(|h| h.set(host));
    unsafe {
        let _ = SetWindowPos(host, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    crate::winbtn::init(hwnd);
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |res, ctrl| {
        if let (Ok(()), Some(c)) = (res, ctrl) {
            setup(c);
        }
        Ok(())
    }));
    unsafe {
        let _ = env.CreateCoreWebView2Controller(if host.is_invalid() { hwnd } else { host }, &handler);
    }
}

fn setup(c: ICoreWebView2Controller) {
    let Ok(wv) = (unsafe { c.CoreWebView2() }) else { return };
    let r = app::with(|s| (bar_rect(s), s.fullscreen.is_none(), s.ui.bg));
    let Some((rect, visible, bg)) = r else { return };
    unsafe {
        if let Ok(c2) = c.cast::<ICoreWebView2Controller2>() {
            let b = bg.0;
            let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: b as u8, G: (b >> 8) as u8, B: (b >> 16) as u8 });
        }
        let _ = c.SetBounds(rect);
        let _ = c.SetIsVisible(visible);
        if let Ok(st) = wv.Settings() {
            let _ = st.SetIsStatusBarEnabled(false);
            let _ = st.SetAreDefaultContextMenusEnabled(false);
            let _ = st.SetIsZoomControlEnabled(false);
            let _ = st.SetAreDevToolsEnabled(true);
            if let Ok(s3) = st.cast::<ICoreWebView2Settings3>() {
                let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
            }
        }
        let mut token = 0i64;
        serve_ui_files(&wv);
        let _ = wv.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut p = PWSTR::null();
                let _ = args.WebMessageAsJson(&mut p);
                on_message(&util::take_pwstr(p));
                Ok(())
            })),
            &mut token,
        );
        // Shortcuts (Ctrl+T, Ctrl+W, ...) also work while the bar has focus.
        let _ = c.add_AcceleratorKeyPressed(&crate::webview::accelerator_handler(), &mut token);
        // Bar links must never open new windows.
        let _ = wv.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(a) = args {
                    let _ = a.SetHandled(true);
                }
                Ok(())
            })),
            &mut token,
        );
        let _ = wv.Navigate(&HSTRING::from(scripts::UI_URL));
    }
    app::with(|s| {
        s.ui.controller = Some(c.clone());
        s.ui.webview = Some(wv.clone());
    });
}

/// Serves https://suppa.ui/* (bar, menus) from the scripts folder.
pub fn serve_ui_files(wv: &ICoreWebView2) {
    let mut token = 0i64;
    unsafe {
        let _ = wv.AddWebResourceRequestedFilter(w!("https://suppa.ui/*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL);
        let _ = wv.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let req = args.Request()?;
                let mut p = PWSTR::null();
                req.Uri(&mut p)?;
                let uri = util::take_pwstr(p);
                let env = app::with(|s| s.env.clone()).flatten();
                if let Some(env) = env {
                    let (status, mime, body) = match scripts::serve(&uri) {
                        Some((m, b)) => (200, m, b),
                        None => (404, "text/plain", b"not found".to_vec()),
                    };
                    let stream = windows::Win32::UI::Shell::SHCreateMemStream(Some(&body));
                    if let Ok(resp) = env.CreateWebResourceResponse(
                        stream.as_ref(),
                        status,
                        w!("OK"),
                        &HSTRING::from(format!("Content-Type: {mime}\r\nCache-Control: no-store")),
                    ) {
                        let _ = args.SetResponse(&resp);
                    }
                }
                Ok(())
            })),
            &mut token,
        );
    }
}

/// Sends a JSON message to the bar.
pub fn post(json: String) {
    post_json(json);
}

/// Bar CSS pixel position -> screen pixels.
fn bar_to_screen(x: f64, y: f64) -> POINT {
    let r = app::with(|s| (s.ui.controller.clone(), bar_rect(s), s.hwnd, zoom(s)));
    let Some((c, rect, hwnd, z)) = r else { return POINT::default() };
    let mut scale = 1.0f64;
    unsafe {
        if let Some(c3) = c.and_then(|c| c.cast::<ICoreWebView2Controller3>().ok()) {
            let _ = c3.RasterizationScale(&mut scale);
        }
    }
    let scale = scale * z;
    let mut pt = POINT { x: rect.left + (x * scale) as i32, y: rect.top + (y * scale) as i32 };
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut pt);
    }
    pt
}

/// Screen pixels -> bar CSS pixels, if the point is over the bar.
pub fn screen_to_bar(pt: POINT) -> Option<(f64, f64)> {
    let (c, rect, hwnd, z) = app::with(|s| (s.ui.controller.clone(), bar_rect(s), s.hwnd, zoom(s)))?;
    let mut scale = 1.0f64;
    let mut p = pt;
    unsafe {
        if let Some(c3) = c.and_then(|c| c.cast::<ICoreWebView2Controller3>().ok()) {
            let _ = c3.RasterizationScale(&mut scale);
        }
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
    }
    let inside = p.x >= rect.left && p.x < rect.right && p.y >= rect.top && p.y < rect.bottom;
    // Not over the page (the bar lies behind it there).
    let page = app::with(|s| content_rect(s)).unwrap_or_default();
    let inside = inside && !(p.x >= page.left && p.x < page.right && p.y >= page.top && p.y < page.bottom);
    let scale = scale * z;
    inside.then(|| ((p.x - rect.left) as f64 / scale, (p.y - rect.top) as f64 / scale))
}

/// Screen point under the ≡ button (right edge of the bar) for notices.
pub fn corner_anchor() -> POINT {
    // Top-right corner of the page area.
    let Some((rc, h, right)) = app::with(|s| (content_rect(s), s.hwnd, scaled(12, s))) else {
        return POINT::default();
    };
    let mut pt = POINT { x: rc.right - right, y: rc.top + right / 2 };
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(h, &mut pt);
    }
    pt
}

/// Keyboard focus to the bar (e.g. for renaming a saved tab).
pub fn focus_bar() {
    if let Some(Some(c)) = app::with(|s| s.ui.controller.clone()) {
        unsafe {
            let _ = c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
    }
}

fn post_json(json: String) {
    if let Some(Some(wv)) = app::with(|s| if s.ui.ready { s.ui.webview.clone() } else { None }) {
        unsafe {
            let _ = wv.PostWebMessageAsJson(&HSTRING::from(json));
        }
    }
}

pub fn command_for(name: &str) -> Option<u16> {
    Some(match name {
        "new_tab" => CMD_NEW_TAB,
        "close_tab" => CMD_CLOSE_TAB,
        "back" => CMD_BACK,
        "forward" => CMD_FORWARD,
        "reload" => CMD_RELOAD,
        "home" => CMD_HOME,
        "update" => CMD_UPDATE,
        "log" => CMD_OPEN_LOG,
        "minimize" => CMD_MINIMIZE,
        "maximize" => CMD_MAXIMIZE,
        "close_window" => CMD_CLOSE_WINDOW,
        "settings" => CMD_SETTINGS,
        "incognito" => CMD_NEW_INCOGNITO,
        "theme" => CMD_TOGGLE_THEME,
        "reopen" => CMD_REOPEN_TAB,
        "fullscreen" => CMD_FULLSCREEN,
        "downloads" => CMD_DOWNLOADS,
        _ => return None,
    })
}

thread_local! {
    static MENU_AT: Cell<POINT> = const { Cell::new(POINT { x: 0, y: 0 }) };
    static SYNC_PENDING: Cell<bool> = const { Cell::new(false) };
    static DEFAULT_ASKED: Cell<bool> = const { Cell::new(false) };
}

/// Screen position where the last requested menu should open.
pub fn menu_anchor() -> POINT {
    MENU_AT.with(|m| m.get())
}

fn on_message(json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
    let hwnd = app::hwnd();
    let s_str = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    match v.get("cmd").and_then(|c| c.as_str()).unwrap_or("") {
        "ready" => {
            app::with(|s| s.ui.ready = true);
            if !DEFAULT_ASKED.with(|a| a.get()) {
                util::log(&format!("startup: bar ready at {} ms", util::since_start_ms()));
            }
            push_theme();
            layout(); // applies the interface scale to the bar
            sync_now();
            post_update();
            crate::saved::sync();
            crate::extensions::toolbar_sync();
            // Once per start (not after an Update restart): offer to become the default browser.
            let first = !DEFAULT_ASKED.with(|a| a.replace(true));
            if first && !std::env::args().any(|a| a == "--restart" || a == "--window") {
                crate::default_browser::maybe_ask(corner_anchor());
            }
        }
        "drag" => {
            // Hit-test codes: caption = move the window, top edges = resize.
            let ht = match s_str("edge").as_str() {
                "top" => 12usize,     // HTTOP
                "topleft" => 13,      // HTTOPLEFT
                "topright" => 14,     // HTTOPRIGHT
                _ => 2,               // HTCAPTION
            };
            util::post(hwnd, WM_APP_NC_DRAG, ht, 0);
        }
        "navigate" => {
            tabs::navigate_active(&s_str("text"));
            focus_webview();
        }
        "activate" => tabs::activate(num("index") as usize),
        "close" => {
            let i = num("index") as usize;
            util::post(hwnd, WM_APP_CLOSE_TAB_INDEX, i, 0);
        }
        "run" => {
            if let Some(cmd) = command_for(&s_str("name")) {
                util::post(hwnd, WM_APP_COMMAND, cmd as usize, 0);
            }
        }
        "star" => crate::saved::toggle_active(),
        "ext_click" => crate::extensions::toolbar_click(&s_str("id"), bar_to_screen(num("x"), num("y"))),
        "ext_menu" => crate::extensions::toolbar_menu(&s_str("id"), bar_to_screen(num("x"), num("y"))),
        "move_tab" => tabs::move_tab(num("from") as usize, num("to") as usize),
        // Tab groups
        "tab_group" => crate::groups::group_with(num("from") as usize, num("target") as usize),
        "group_toggle" => crate::groups::toggle(num("id") as u32),
        "group_menu" => crate::groups::menu(num("id") as u32, bar_to_screen(num("x"), num("y"))),
        "group_rename" => crate::groups::rename(num("id") as u32, &s_str("name")),
        "tab_detach" => tabs::detach(num("index") as usize),
        "rows" => {
            // Row heights dragged in the bar (live while dragging; saved at the end).
            use crate::settings::{ROW_NAV, ROW_SAVED, ROW_TABS, clamp_row};
            let save = v.get("save").and_then(|b| b.as_bool()).unwrap_or(false);
            app::with(|s| {
                if s.settings.lock_sizes {
                    return;
                }
                s.settings.row_tabs = clamp_row(num("tabs") as i32, ROW_TABS);
                s.settings.row_nav = clamp_row(num("nav") as i32, ROW_NAV);
                s.settings.row_saved = clamp_row(num("saved") as i32, ROW_SAVED);
                // Width of the side bar (bar on the left/right)
                if v.get("side").is_some() {
                    s.settings.side_width = clamp_row(num("side") as i32, crate::settings::SIDE_W);
                }
                if save {
                    settings::save(&s.settings);
                }
            });
            layout();
        }
        "accent" => {
            let c = s_str("color");
            app::with(|s| s.ui.accent = c);
        }
        "tab_menu" => {
            let pt = bar_to_screen(num("x"), num("y"));
            crate::menu::show_tab(num("index") as usize, pt);
        }
        c @ ("saved_open" | "saved_folder" | "saved_menu" | "saved_move" | "saved_rename") => {
            let pt = bar_to_screen(num("x"), num("y"));
            crate::saved::on_message(c, &v, pt);
        }
        "menu" => {
            // CSS px in the bar -> screen px.
            MENU_AT.with(|m| m.set(bar_to_screen(num("x"), num("y"))));
            let cmd = match s_str("which").as_str() {
                "extensions" => CMD_EXT_MENU,
                "adblock" => CMD_ADBLOCK_MENU,
                _ => CMD_MENU,
            };
            util::post(hwnd, WM_APP_COMMAND, cmd as usize, 0);
        }
        "set_theme" => {
            let dark = v.get("dark").and_then(|d| d.as_bool()).unwrap_or(true);
            let (dbg, dborder) = default_colors(dark);
            let bg = parse_hex(&s_str("bg")).unwrap_or(dbg);
            let border = parse_hex(&s_str("border")).unwrap_or(dborder);
            app::with(|s| {
                s.ui.dark = dark;
                s.ui.bg = bg;
                s.ui.border = border;
                if s.settings.dark != dark {
                    s.settings.set_dark(dark);
                }
                settings::save(&s.settings);
            });
            apply_window_theme(hwnd, dark);
            crate::winbtn::refresh();
            let host = HOST.with(|h| h.get());
            if !host.is_invalid() {
                unsafe {
                    let _ = InvalidateRect(Some(host), None, true);
                }
            }
        }
        "focus_page" => focus_webview(),
        _ => {}
    }
}

/// Sends theme + accent colour to the bar (it answers with set_theme).
pub fn push_theme() {
    let r = app::with(|s| {
        let st = &s.settings;
        serde_json::json!({
            "type": "init", "dark": st.dark, "accent": accent_for_bar(st), "saved": st.show_saved,
            "rows": [st.row_tabs, st.row_nav, st.row_saved], "locked": st.lock_sizes, "radius": st.radius,
            "colors": colors_json(st),
            "on_accent": st.on_accent,
            "order": st.row_order, "bar_pos": st.bar_pos, "side_width": st.side_width,
            "pos": { "tabs": st.tabs_pos, "nav": st.nav_pos, "saved": st.saved_pos },
            "tab_corners": st.tab_corners, "squircle": st.corner_shape == "squircle",
            "theme": st.theme_name(), "icons": st.icon_pack,
        })
        .to_string()
    });
    if let Some(json) = r {
        post_json(json);
    }
}

/// The accent colour chosen in Windows settings (Personalization -> Colors).
pub fn windows_accent() -> String {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let mut v = 0u32;
    let mut size = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
        .is_ok()
    };
    // Stored as 0xAABBGGRR.
    if ok { format!("#{:02x}{:02x}{:02x}", v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff) } else { "#0078d4".to_string() }
}

/// Accent setting as the bar understands it: "cycle" or a colour.
pub fn accent_for_bar(st: &crate::settings::Settings) -> String {
    if st.accent == "windows" { windows_accent() } else { st.accent.clone() }
}

/// Custom colours of the current theme as {"bg": "#101010", ...} (Settings -> Colors).
pub fn custom_colors_json(st: &crate::settings::Settings) -> serde_json::Value {
    serde_json::Value::Object(st.colors().iter().map(|(n, h)| (n.clone(), serde_json::Value::String(h.clone()))).collect())
}

/// Every interface colour as {"bg": "#101010", ...}: the theme (dark, darker,
/// light, lighter), mixed with the tint colour, then the custom colours on top.
/// The bar, menus and the Settings page all paint with these.
pub fn colors_json(st: &crate::settings::Settings) -> serde_json::Value {
    serde_json::Value::Object(palette(st).into_iter().map(|(n, h)| (n.to_string(), serde_json::Value::String(h))).collect())
}

const PALETTES: [(&str, [u32; 11]); 4] = [
    //            bg        panel     field     btn       btn-hover tab       tab-fg    fg        muted     border    incognito
    ("dark", [0x161616, 0x1f1f1f, 0x272727, 0x2e2e2e, 0x3b3b3b, 0x2e2e2e, 0x8e8e8e, 0xececec, 0x8e8e8e, 0x383838, 0x5b4a86]),
    ("darker", [0x000000, 0x0c0c0c, 0x161616, 0x1c1c1c, 0x2a2a2a, 0x1c1c1c, 0x8a8a8a, 0xf2f2f2, 0x8a8a8a, 0x262626, 0x463a6e]),
    ("light", [0xf2f2f2, 0xffffff, 0xffffff, 0xe5e5e5, 0xd8d8d8, 0xe5e5e5, 0x6b6b6b, 0x1c1c1c, 0x6b6b6b, 0xd0d0d0, 0x6f5aa8]),
    ("lighter", [0xf8f8f8, 0xffffff, 0xf6f6f6, 0xf0f0f0, 0xe6e6e6, 0xececec, 0x707070, 0x111111, 0x707070, 0xe2e2e2, 0x7c66c2]),
];

/// See colors_json.
pub fn palette(st: &crate::settings::Settings) -> Vec<(&'static str, String)> {
    let theme = st.theme_name();
    let base = PALETTES.iter().find(|(n, _)| *n == theme).map(|(_, p)| *p).unwrap_or(PALETTES[0].1);
    let tint = u32::from_str_radix(st.tint.trim_start_matches('#'), 16).unwrap_or(0xff0000);
    let amount = st.tint_amount.clamp(0, 100) as f64 / 100.0;
    let mix = |c: u32, a: f64| -> String {
        let ch = |shift: u32| {
            let x = ((c >> shift) & 255) as f64;
            let t = ((tint >> shift) & 255) as f64;
            (x + (t - x) * a).round() as u32
        };
        format!("#{:02x}{:02x}{:02x}", ch(16), ch(8), ch(0))
    };
    let custom = st.colors();
    let get = |n: &str| custom.iter().find(|(k, _)| k == n).map(|(_, h)| h.clone());
    settings::COLOR_NAMES
        .iter()
        .zip(base)
        .map(|(&name, c)| {
            // Text is tinted less, so it stays easy to read.
            let a = if matches!(name, "fg" | "muted" | "tab-fg") { amount * 0.35 } else { amount };
            // Tabs follow a custom button / secondary text colour unless set themselves.
            let own = get(name).or_else(|| match name {
                "tab" => get("btn"),
                "tab-fg" => get("muted"),
                _ => None,
            });
            (name, own.unwrap_or_else(|| mix(c, a)))
        })
        .collect()
}

/// A palette colour as a COLORREF (for things Windows draws, e.g. the window buttons).
pub fn palette_color(st: &crate::settings::Settings, name: &str) -> COLORREF {
    palette(st).into_iter().find(|(n, _)| *n == name).and_then(|(_, h)| parse_hex(&h)).unwrap_or(rgb(0x80, 0x80, 0x80))
}

/// Update state of the ≡ button ("Update ≡" / "Building… ≡").
pub fn post_update() {
    if let Some((avail, building)) = app::with(|s| (s.ui.update_available, s.building)) {
        post_json(format!("{{\"type\":\"update\",\"available\":{avail},\"building\":{building}}}"));
    }
}

/// Sends tabs + address bar state to the bar (coalesced: at most once per message loop turn).
pub fn request_sync() {
    if !SYNC_PENDING.with(|p| p.replace(true)) {
        let ok = unsafe {
            PostMessageW(Some(app::hwnd()), WM_APP_UI_SYNC, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0))
        };
        if ok.is_err() {
            // Never leave the flag stuck, or the bar would stop updating.
            SYNC_PENDING.with(|p| p.set(false));
        }
    }
}

pub fn sync_now() {
    SYNC_PENDING.with(|p| p.set(false));
    let json = app::with(|s| {
        let tabs: Vec<serde_json::Value> = s
            .tabs
            .iter()
            .map(|t| {
                let state = if t.loading {
                    "loading"
                } else if t.controller.is_none() && !t.creating && !newtab::is_newtab(&t.url) {
                    "unloaded"
                } else if t.suspended {
                    "sleep"
                } else {
                    ""
                };
                // No favicon request from the bar for incognito tabs.
                let fav = if t.incognito || !t.favicon.starts_with("http") { "" } else { t.favicon.as_str() };
                serde_json::json!({ "id": t.id, "t": tab_title(t), "s": state, "i": t.incognito, "f": fav, "g": t.group })
            })
            .collect();
        let maximized = unsafe { IsZoomed(s.hwnd).as_bool() };
        let t = s.active_tab();
        let url = t.map(|t| if newtab::is_newtab(&t.url) { String::new() } else { t.url.clone() }).unwrap_or_default();
        let host = t.map(|t| util::host_of(&t.doc_url)).unwrap_or_default();
        let ads = if !s.settings.adblock {
            "off"
        } else if s.settings.is_allowed(&host) {
            "site"
        } else {
            "on"
        };
        let (mut back, mut fwd) = (BOOL(0), BOOL(0));
        if let Some(wv) = t.and_then(|t| t.webview.as_ref()) {
            unsafe {
                let _ = wv.CanGoBack(&mut back);
                let _ = wv.CanGoForward(&mut fwd);
            }
        }
        let title = match t {
            Some(t) if !t.title.is_empty() => format!("{} - Suppa", t.title),
            _ => "Suppa".to_string(),
        };
        unsafe {
            let _ = SetWindowTextW(s.hwnd, &HSTRING::from(title));
        }
        serde_json::json!({
            "type": "sync",
            "tabs": tabs,
            "active": s.active,
            "url": url,
            "blocked": t.map(|t| t.blocked).unwrap_or(0),
            "ads": ads,
            "back": back.as_bool(),
            "fwd": fwd.as_bool(),
            "max": maximized,
            "inc": t.map(|t| t.incognito).unwrap_or(false),
            "groups": crate::groups::json(s),
            "star": !url.is_empty() && crate::saved::contains(&url),
        })
        .to_string()
    });
    if let Some(json) = json {
        post_json(json);
    }
}

pub fn tab_title(t: &app::Tab) -> String {
    if !t.title.is_empty() {
        t.title.clone()
    } else if newtab::is_newtab(&t.url) {
        "New Tab".to_string()
    } else {
        util::host_of(&t.url)
    }
}

pub fn set_update_available(on: bool) {
    let changed = app::with(|s| std::mem::replace(&mut s.ui.update_available, on) != on).unwrap_or(false);
    if changed {
        if on {
            util::log("scripts folder / Rust code changed - Update button lit");
        }
        post_update();
    }
}

// ---- Hooks used by tabs.rs / webview.rs (all just schedule a bar refresh) -------------

pub fn insert_tab_item(_s: &app::State, _idx: usize) {
    request_sync();
}
pub fn update_tab_item(_s: &app::State, _idx: usize) {
    request_sync();
}
pub fn update_tab_by_id(_s: &app::State, _id: u32) {
    request_sync();
}
pub fn remove_tab_item(_s: &app::State, _idx: usize) {
    request_sync();
}
pub fn select_tab_item(_s: &app::State, _idx: usize) {
    request_sync();
}
pub fn refresh_toolbar(_s: &app::State) {
    request_sync();
}

// ---- Layout & focus ---------------------------------------------------------------------

/// Positions the bar and the active page.
pub fn layout() {
    let r = app::with(|s| {
        (
            s.ui.controller.clone(),
            bar_rect(s),
            s.fullscreen.is_none() && !s.minimized,
            s.active_tab().and_then(|t| t.controller.clone()),
            content_rect(s),
            s.hwnd,
            zoom(s),
        )
    });
    let Some((bar, bar_rc, bar_visible, page, page_rc, hwnd, bar_zoom)) = r else { return };
    let host = HOST.with(|h| h.get());
    unsafe {
        // The bar's window covers the whole window, under the page.
        let bar_rc = if host.is_invalid() {
            bar_rc
        } else {
            let _ = SetWindowPos(
                host,
                Some(HWND_BOTTOM),
                0,
                0,
                bar_rc.right,
                bar_rc.bottom,
                SWP_NOACTIVATE | if bar_visible { SWP_SHOWWINDOW } else { SWP_HIDEWINDOW },
            );
            RECT { left: 0, top: 0, right: bar_rc.right, bottom: bar_rc.bottom }
        };
        if let Some(b) = bar {
            let _ = b.SetBounds(bar_rc);
            let _ = b.SetIsVisible(bar_visible);
            let _ = b.SetZoomFactor(bar_zoom); // Interface scale
        }
        if let Some(p) = page {
            let _ = p.SetBounds(page_rc);
        }
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
    crate::winbtn::layout();
}

pub fn on_dpi_changed(s: &mut app::State, dpi: u32) {
    s.dpi = dpi;
}

pub fn focus_address() {
    let c = app::with(|s| s.ui.controller.clone()).flatten();
    if let Some(c) = c {
        unsafe {
            let _ = c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
        post_json("{\"type\":\"focus_address\"}".into());
    }
}

pub fn focus_webview() {
    if let Some(Some(c)) = app::with(|s| s.active_tab().and_then(|t| t.controller.clone())) {
        unsafe {
            let _ = c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
    }
}
