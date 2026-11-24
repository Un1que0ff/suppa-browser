// Themed popup menus. Every menu (≡, tab, saved tabs, folders, page right-click)
// is drawn by scripts\menu.html in a small popup window that is created once at
// startup and reused, so menus open instantly and follow the browser theme.
use std::cell::RefCell;
use std::time::Instant;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PWSTR, w};

use crate::{app, ui, util};

const WM_POPUP_DISMISS: u32 = WM_APP + 50;
pub const MENU_URL: &str = "https://suppa.ui/menu.html";

// ---- Menu items ---------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Item {
    pub id: i64,
    pub label: String,
    pub key: String,
    pub icon: String,
    pub checked: bool,
    pub enabled: bool,
    pub sep: bool,
    pub hot: bool,
    /// Can be dragged to a new position (folder contents).
    pub drag: bool,
    pub sub: Vec<Item>,
    /// Right-click actions for this item.
    pub ctx: Vec<Item>,
    /// Dialog text: bold heading / wrapping paragraph (not clickable).
    pub heading: bool,
    pub para: bool,
}

impl Item {
    pub fn new(id: i64, label: impl Into<String>) -> Self {
        Item { id, label: label.into(), enabled: true, ..Default::default() }
    }
    pub fn cmd(id: u16, label: impl Into<String>) -> Self {
        Item::new(id as i64, label)
    }
    pub fn sep() -> Self {
        Item { sep: true, ..Default::default() }
    }
    pub fn sub(label: impl Into<String>, items: Vec<Item>) -> Self {
        Item { label: label.into(), enabled: true, sub: items, id: -1, ..Default::default() }
    }
    /// Bold dialog title.
    pub fn heading(text: impl Into<String>) -> Self {
        Item { label: text.into(), heading: true, ..Default::default() }
    }
    /// Wrapping dialog text.
    pub fn text(text: impl Into<String>) -> Self {
        Item { label: text.into(), para: true, ..Default::default() }
    }
    pub fn key(mut self, k: &str) -> Self {
        self.key = k.to_string();
        self
    }
    pub fn checked(mut self, c: bool) -> Self {
        self.checked = c;
        self
    }
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }
    pub fn icon(mut self, i: &str) -> Self {
        self.icon = i.to_string();
        self
    }
    pub fn hot(mut self, h: bool) -> Self {
        self.hot = h;
        self
    }
    pub fn draggable(mut self) -> Self {
        self.drag = true;
        self
    }
    pub fn ctx(mut self, items: Vec<Item>) -> Self {
        self.ctx = items;
        self
    }
    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id, "l": self.label, "k": self.key, "i": self.icon,
            "c": self.checked, "e": self.enabled, "s": self.sep, "h": self.hot, "d": self.drag,
            "H": self.heading, "P": self.para,
            "sub": self.sub.iter().map(Item::json).collect::<Vec<_>>(),
            "ctx": self.ctx.iter().map(Item::json).collect::<Vec<_>>(),
        })
    }
}

fn items_json(items: &[Item]) -> serde_json::Value {
    serde_json::Value::Array(items.iter().map(Item::json).collect())
}

// ---- Popup window ---------------------------------------------------------------------

type PickFn = Box<dyn FnOnce(Option<i64>)>;
type ReorderFn = Box<dyn FnMut(usize, usize)>;

#[derive(Default)]
struct Popup {
    hwnd: HWND,
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    ready: bool,
    queued: Option<String>,
    on_pick: Option<PickFn>,
    on_reorder: Option<ReorderFn>,
    anchor: POINT,
    right: bool,
    /// Dialog: centred on the browser window instead of hanging from `anchor`.
    center: bool,
    key: String,
    visible: bool,
    /// Which menu was closed by clicking outside, and when (so clicking its
    /// button again closes the menu instead of reopening it).
    hidden: Option<(String, Instant)>,
}

thread_local! {
    static P: RefCell<Popup> = RefCell::new(Popup::default());
}

fn with<R>(f: impl FnOnce(&mut Popup) -> R) -> Option<R> {
    P.with(|p| p.try_borrow_mut().ok().map(|mut g| f(&mut g)))
}

/// Creates the (hidden) popup window. Called once after the main window exists.
pub fn init(owner: HWND) {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("SuppaPopup");
        // No CS_DROPSHADOW: Windows draws that shadow as a square box offset to
        // the bottom-right, which looks broken around the rounded menu.
        let wc = WNDCLASSW {
            lpfnWndProc: Some(popup_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        if let Ok(h) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class,
            w!("Suppa menu"),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            10,
            10,
            Some(owner),
            None,
            Some(hinst.into()),
            None,
        ) {
            with(|p| p.hwnd = h);
        }
    }
}

/// Creates the menu WebView (once the WebView2 environment exists).
pub fn create(env: &ICoreWebView2Environment) {
    let Some(hwnd) = with(|p| p.hwnd) else { return };
    if hwnd.is_invalid() {
        return;
    }
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |res, ctrl| {
        if let (Ok(()), Some(c)) = (res, ctrl) {
            setup(c);
        }
        Ok(())
    }));
    unsafe {
        let _ = env.CreateCoreWebView2Controller(hwnd, &handler);
    }
}

fn setup(c: ICoreWebView2Controller) {
    let Ok(wv) = (unsafe { c.CoreWebView2() }) else { return };
    unsafe {
        if let Ok(st) = wv.Settings() {
            let _ = st.SetIsStatusBarEnabled(false);
            let _ = st.SetAreDefaultContextMenusEnabled(false);
            let _ = st.SetIsZoomControlEnabled(false);
            if let Ok(s3) = windows::core::Interface::cast::<ICoreWebView2Settings3>(&st) {
                let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
            }
        }
        ui::serve_ui_files(&wv);
        let mut token = 0i64;
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
        // Also "ready" when WebView2 says the page finished loading: menus never
        // depend on the page's own ready message alone.
        let _ = wv.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(|_, args| {
                let mut ok = windows::core::BOOL(0);
                if let Some(a) = args {
                    let _ = a.IsSuccess(&mut ok);
                }
                if ok.as_bool() {
                    page_ready();
                }
                Ok(())
            })),
            &mut token,
        );
        let _ = wv.Navigate(&HSTRING::from(MENU_URL));
    }
    // Hidden until the first menu: low memory from the start.
    crate::webview::set_memory_low(&c, true);
    with(|p| {
        p.controller = Some(c);
        p.webview = Some(wv);
    });
}

fn post(json: String) {
    if let Some(Some(wv)) = with(|p| p.webview.clone()) {
        unsafe {
            let _ = wv.PostWebMessageAsJson(&HSTRING::from(json));
        }
    }
}

fn theme_json() -> (bool, String) {
    app::with(|s| (s.settings.dark, s.ui.accent.clone())).unwrap_or((true, "#b07cff".into()))
}

/// Corner radius of menus/dialogs in CSS px: the roundness setting + 2 (0 = square).
fn menu_radius() -> i32 {
    let r = app::with(|s| s.settings.radius).unwrap_or(10);
    if r <= 0 { 0 } else { r + 2 }
}

/// Shows a menu. `anchor` is in screen pixels; `right` = the menu's right edge
/// is at anchor.x (else its left edge). `on_pick` gets the chosen id, or None
/// when the menu is closed without a choice. It is always called exactly once.
pub fn show(key: &str, items: Vec<Item>, anchor: POINT, right: bool, on_pick: impl FnOnce(Option<i64>) + 'static) {
    show_ex(key, items, anchor, right, Box::new(on_pick), None);
}

/// Like `show`, and items marked `draggable` can be reordered (from, to).
pub fn show_reorder(
    key: &str,
    items: Vec<Item>,
    anchor: POINT,
    right: bool,
    on_pick: impl FnOnce(Option<i64>) + 'static,
    on_reorder: impl FnMut(usize, usize) + 'static,
) {
    show_ex(key, items, anchor, right, Box::new(on_pick), Some(Box::new(on_reorder)));
}

fn show_ex(key: &str, items: Vec<Item>, anchor: POINT, right: bool, on_pick: PickFn, on_reorder: Option<ReorderFn>) {
    let recently = with(|p| matches!(&p.hidden, Some((k, t)) if k == key && t.elapsed().as_millis() < 350)).unwrap_or(false);
    if recently {
        with(|p| p.hidden = None);
        on_pick(None);
        return;
    }
    // Another menu is open: it closes without a choice.
    if let Some(Some(old)) = with(|p| p.on_pick.take()) {
        old(None);
    }
    // Normal memory while a menu is open; same interface scale as the bar.
    if let Some(Some(c)) = with(|p| p.controller.clone()) {
        crate::webview::set_memory_low(&c, false);
        let z = app::with(|s| ui::zoom(s)).unwrap_or(1.0);
        unsafe {
            let _ = c.SetZoomFactor(z);
        }
    }
    let (dark, accent) = theme_json();
    let msg = serde_json::json!({
        "type": "show", "items": items_json(&items), "dark": dark, "accent": accent,
        "radius": app::with(|s| s.settings.radius).unwrap_or(10),
        "colors": app::with(|s| ui::colors_json(&s.settings)).unwrap_or_default(),
        "on_accent": app::with(|s| s.settings.on_accent.clone()).unwrap_or_default(),
    })
    .to_string();
    let ready = with(|p| {
        p.on_pick = Some(on_pick);
        p.on_reorder = on_reorder;
        p.anchor = anchor;
        p.right = right;
        p.center = key.starts_with("dialog");
        p.key = key.to_string();
        p.ready
    })
    .unwrap_or(false);
    if !ready {
        // Kept until the page says it is loaded ...
        with(|p| p.queued = Some(msg.clone()));
    }
    // ... and sent now as well: if the page is loaded but its "ready" got lost,
    // the menu still opens (a page that is not loaded yet just ignores it).
    post(msg);
}

/// The menu page is loaded: a menu that was asked for meanwhile is sent.
fn page_ready() {
    if let Some(Some(q)) = with(|p| {
        p.ready = true;
        let q = p.queued.take();
        // Only if that menu is still wanted (not closed in the meantime).
        if p.on_pick.is_some() { q } else { None }
    }) {
        post(q);
    }
}

/// `on_reorder(from, DROPPED_OUTSIDE)`: an item was dragged out of the menu.
pub const DROPPED_OUTSIDE: usize = usize::MAX;

/// Closes the open menu (no choice).
pub fn close() {
    if is_open() {
        finish(None, true);
    }
}

/// Themed Yes/No dialog in the middle of the browser. `on_yes` runs only on Yes
/// (Esc, No or clicking elsewhere = No).
pub fn confirm(title: &str, text: &str, yes: &str, no: &str, on_yes: impl FnOnce() + 'static) {
    let mut items = vec![Item::heading(title)];
    if !text.is_empty() {
        items.push(Item::text(text));
    }
    items.push(Item::sep());
    items.push(Item::new(1, yes).hot(true));
    items.push(Item::new(2, no));
    show("dialog-confirm", items, POINT::default(), false, move |pick| {
        if pick == Some(1) {
            on_yes();
        }
    });
}

/// Themed message with an OK button. The first line is the title.
pub fn message(text: &str) {
    let (title, body) = match text.split_once("\n\n").or_else(|| text.split_once('\n')) {
        Some((t, b)) => (t.trim(), b.trim()),
        None => (text.trim(), ""),
    };
    let mut items = vec![Item::heading(title)];
    if !body.is_empty() {
        items.push(Item::text(body));
    }
    items.push(Item::sep());
    items.push(Item::new(1, "OK").hot(true));
    show("dialog-message", items, POINT::default(), false, |_| {});
}

/// Replaces the items of the open menu (e.g. after a reorder).
pub fn refresh(items: Vec<Item>) {
    post(serde_json::json!({ "type": "items", "items": items_json(&items) }).to_string());
}

fn on_message(json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    match v.get("cmd").and_then(|c| c.as_str()).unwrap_or("") {
        "ready" => page_ready(),
        "size" => place(num("w"), num("h")),
        "pick" => finish(Some(num("id") as i64), true),
        "close" => finish(None, true),
        c @ ("reorder" | "drag_out") => {
            if let Some(Some(mut f)) = with(|p| p.on_reorder.take()) {
                // to == DROPPED_OUTSIDE: the item was let go outside the menu.
                let to = if c == "drag_out" { DROPPED_OUTSIDE } else { num("to") as usize };
                f(num("from") as usize, to);
                with(|p| {
                    if p.on_reorder.is_none() && p.visible {
                        p.on_reorder = Some(f);
                    }
                });
            }
        }
        _ => {}
    }
}

/// Positions the popup for content of w x h CSS pixels and shows it.
fn place(wc: f64, hc: f64) {
    let Some((hwnd, mut anchor, right, ctrl, open, was_visible, center)) =
        with(|p| (p.hwnd, p.anchor, p.right, p.controller.clone(), p.on_pick.is_some(), p.visible, p.center))
    else {
        return;
    };
    if !open || hwnd.is_invalid() || wc < 1.0 {
        return;
    }
    let (dpi, zoom) = app::with(|s| (s.dpi as f64, ui::zoom(s))).unwrap_or((96.0, 1.0));
    // DPI x interface scale (the menu page is zoomed by the same factor).
    let scale = dpi / 96.0 * zoom;
    unsafe {
        let mon = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let wa = mi.rcWork;
        let w = ((wc * scale).ceil() as i32).min(wa.right - wa.left);
        let h = ((hc * scale).ceil() as i32).min(wa.bottom - wa.top);
        if center {
            // Dialog: horizontally centred, a third of the way down the browser window.
            let mut wr = RECT::default();
            let _ = GetWindowRect(app::hwnd(), &mut wr);
            anchor = POINT { x: (wr.left + wr.right) / 2 - w / 2, y: wr.top + ((wr.bottom - wr.top) / 3 - h / 2).max(60) };
        }
        let x = (if right && !center { anchor.x - w } else { anchor.x }).clamp(wa.left, (wa.right - w).max(wa.left));
        let mut y = anchor.y;
        if y + h > wa.bottom {
            y = if anchor.y - h >= wa.top { anchor.y - h } else { wa.bottom - h };
        }
        // CreateRoundRectRgn takes the corner *diameter*: 2 x the CSS radius of the menu box.
        let d = (2.0 * menu_radius() as f64 * scale).round() as i32;
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, d, d);
        SetWindowRgn(hwnd, Some(rgn), true);
        let _ = SetWindowPos(hwnd, Some(HWND_TOP), x, y, w, h, SWP_SHOWWINDOW);
        if let Some(c) = &ctrl {
            let _ = c.SetBounds(RECT { left: 0, top: 0, right: w, bottom: h });
            let _ = c.SetIsVisible(true);
        }
        with(|p| p.visible = true);
        if !was_visible {
            // Watch for clicks outside the menu (works even if Windows did not
            // give the menu window focus, e.g. after a right-click).
            OUTSIDE_PREV.with(|c| c.set(mouse_buttons_down()));
            FG_AT_SHOW.with(|c| c.set(GetForegroundWindow().0 as isize));
            SetTimer(Some(hwnd), OUTSIDE_TIMER, 30, None);
            let _ = SetForegroundWindow(hwnd);
            if let Some(c) = &ctrl {
                let _ = c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
            post("{\"type\":\"focus\"}".into());
        }
    }
}

/// Hides the popup and reports the result to whoever opened it.
fn finish(result: Option<i64>, activate_main: bool) {
    let Some((hwnd, f, ctrl)) = with(|p| {
        p.visible = false;
        p.on_reorder = None;
        p.hidden = if activate_main { None } else { Some((p.key.clone(), Instant::now())) };
        (p.hwnd, p.on_pick.take(), p.controller.clone())
    }) else {
        return;
    };
    unsafe {
        let _ = KillTimer(Some(hwnd), OUTSIDE_TIMER);
        let _ = ShowWindow(hwnd, SW_HIDE);
        // Hidden: no rendering and low memory until the next menu. (Not suspended:
        // a suspended page can miss the next menu's message, and an idle hidden
        // menu page uses no CPU anyway.)
        if let Some(c) = &ctrl {
            let _ = c.SetIsVisible(false);
            crate::webview::set_memory_low(c, true);
        }
        if activate_main {
            let _ = SetForegroundWindow(app::hwnd());
        }
    }
    post("{\"type\":\"hide\"}".into());
    if let Some(f) = f {
        f(result);
    }
}

const OUTSIDE_TIMER: usize = 7;

thread_local! {
    static OUTSIDE_PREV: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Foreground window when the menu opened (switching away from it closes the menu).
    static FG_AT_SHOW: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn mouse_buttons_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
    unsafe { [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| GetAsyncKeyState(k.0 as i32) as u16 & 0x8000 != 0) }
}

/// Timer while a menu is open: a mouse press outside it, or switching to
/// another app, closes it.
fn check_outside(hwnd: HWND) {
    unsafe {
        let down = mouse_buttons_down();
        let pressed = down && !OUTSIDE_PREV.with(|c| c.replace(down));
        if !down {
            OUTSIDE_PREV.with(|c| c.set(false));
        }
        let fg = GetForegroundWindow();
        let other_app = fg != hwnd && fg != app::hwnd() && !fg.is_invalid() && fg.0 as isize != FG_AT_SHOW.with(|c| c.get());
        let mut outside = false;
        if pressed {
            let mut pt = POINT::default();
            let mut rc = RECT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = GetWindowRect(hwnd, &mut rc);
            outside = pt.x < rc.left || pt.x >= rc.right || pt.y < rc.top || pt.y >= rc.bottom;
        }
        if outside || other_app {
            finish(None, false);
        }
    }
}

pub fn is_open() -> bool {
    with(|p| p.visible).unwrap_or(false)
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_ACTIVATE => {
                // Clicked somewhere else: close (posted, never inside SetWindowPos).
                if wp.0 & 0xFFFF == 0 {
                    util::post(hwnd, WM_POPUP_DISMISS, 0, 0);
                }
                LRESULT(0)
            }
            WM_POPUP_DISMISS => {
                if is_open() && GetForegroundWindow() != hwnd {
                    finish(None, false);
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == OUTSIDE_TIMER => {
                if is_open() {
                    check_outside(hwnd);
                } else {
                    let _ = KillTimer(Some(hwnd), OUTSIDE_TIMER);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
