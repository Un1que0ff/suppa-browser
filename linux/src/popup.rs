// Themed popup menus and dialogs. Every menu (≡, tab, saved tabs, folders) is
// drawn by scripts/menu.html in a small borderless window that is created once
// and reused, so menus open instantly and follow the browser theme.
use std::cell::RefCell;

use gtk::prelude::*;
use crate::wk::*;
use serde_json::json;
use webkit2gtk::{LoadEvent, UserContentManager, WebView};

use crate::{app, scripts, ui};

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

/// Mouse position on the screen.
pub fn cursor_pos() -> POINT {
    let Some(d) = gtk::gdk::Display::default() else { return POINT::default() };
    let Some(p) = d.default_seat().and_then(|s| s.pointer()) else { return POINT::default() };
    let (_, x, y) = p.position();
    POINT { x, y }
}

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
    pub drag: bool,
    pub sub: Vec<Item>,
    pub ctx: Vec<Item>,
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
    pub fn heading(text: impl Into<String>) -> Self {
        Item { label: text.into(), heading: true, ..Default::default() }
    }
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
        json!({
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

/// `to` value of a reorder when the item was let go outside the menu.
pub const DROPPED_OUTSIDE: usize = usize::MAX;

struct Popup {
    window: gtk::Window,
    view: WebView,
    ready: bool,
    queued: Option<String>,
    on_pick: Option<PickFn>,
    on_reorder: Option<ReorderFn>,
    anchor: POINT,
    right: bool,
    center: bool,
    /// Ignore focus loss for a moment after opening (the window manager may
    /// hand focus around while mapping the window).
    opened_at: std::time::Instant,
}

thread_local! {
    static POPUP: RefCell<Option<Popup>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Popup) -> R) -> Option<R> {
    POPUP.with(|c| c.try_borrow_mut().ok()?.as_mut().map(f))
}

/// Menus have see-through rounded corners: the page draws the rounded box.
const TRANSPARENT_CSS: &str = "html,body{background:transparent!important}";

fn create() {
    if POPUP.with(|c| c.borrow().is_some()) {
        return;
    }
    let Some((main, ctx)) = app::with(|s| (s.window.clone(), s.ctx.clone())) else { return };
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_decorated(false);
    window.set_skip_taskbar_hint(true);
    window.set_skip_pager_hint(true);
    window.set_type_hint(gtk::gdk::WindowTypeHint::PopupMenu);
    window.set_transient_for(Some(&main));
    window.set_resizable(false);
    window.set_app_paintable(true);
    if let Some(vis) = WidgetExt::screen(&window).and_then(|s| s.rgba_visual()) {
        window.set_visual(Some(&vis));
    }
    let ucm = UserContentManager::new();
    ucm.register_script_message_handler("suppa");
    ucm.add_script(&webkit2gtk::UserScript::new(
        ui::BRIDGE_JS,
        webkit2gtk::UserContentInjectedFrames::TopFrame,
        webkit2gtk::UserScriptInjectionTime::Start,
        &[],
        &[],
    ));
    ucm.add_style_sheet(&webkit2gtk::UserStyleSheet::new(
        TRANSPARENT_CSS,
        webkit2gtk::UserContentInjectedFrames::TopFrame,
        webkit2gtk::UserStyleLevel::User,
        &[],
        &[],
    ));
    let view = WebView::builder().web_context(&ctx).user_content_manager(&ucm).build();
    view.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
    view.connect_context_menu(|_, _, _, _| true);
    view.connect_load_changed(|_, ev| {
        if ev == LoadEvent::Finished {
            let q = with(|p| {
                p.ready = true;
                p.queued.take()
            })
            .flatten();
            if let Some(js) = q {
                send(js);
            }
        }
    });
    ucm.connect_script_message_received(Some("suppa"), |_, msg| {
        let text = msg.js_value().map(|v| v.to_str().to_string()).unwrap_or_default();
        app::later(move || on_message(&text));
    });
    window.add(&view);
    view.show();
    window.connect_focus_out_event(|w, _| {
        // Clicked outside the open menu. (Hiding it after a pick also takes the
        // focus away: that must not close the dialog the pick may open next.)
        let fresh = with(|p| p.opened_at.elapsed().as_millis() < 250).unwrap_or(false);
        if !fresh && w.is_visible() {
            app::later(|| dismiss());
        }
        gtk::glib::Propagation::Proceed
    });
    window.connect_delete_event(|w, _| {
        w.hide();
        app::later(|| dismiss());
        gtk::glib::Propagation::Stop
    });
    view.load_uri(scripts::MENU_URL);
    POPUP.with(|c| {
        *c.borrow_mut() = Some(Popup {
            window,
            view,
            ready: false,
            queued: None,
            on_pick: None,
            on_reorder: None,
            anchor: POINT::default(),
            right: false,
            center: false,
            opened_at: std::time::Instant::now(),
        })
    });
}

/// Creates the menu window early so the first menu opens instantly.
pub fn preload() {
    create();
}

fn send(msg: String) {
    let r = with(|p| {
        if p.ready {
            Some(p.view.clone())
        } else {
            p.queued = Some(msg.clone());
            None
        }
    })
    .flatten();
    if let Some(v) = r {
        crate::tabs::run_js(&v, &format!("window.__suppaRecv({msg})"));
    }
}

/// Shows a menu. `anchor` is in screen pixels; `right` = the menu's right edge
/// is at anchor.x (else its left edge). `on_pick` gets the chosen id, or None
/// when the menu was closed without a choice.
pub fn show(key: &str, items: Vec<Item>, anchor: POINT, right: bool, on_pick: impl FnOnce(Option<i64>) + 'static) {
    show_ex(key, items, anchor, right, Box::new(on_pick), None);
}

/// Like `show`, but items marked draggable can be dragged to a new place.
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

fn show_ex(_key: &str, items: Vec<Item>, anchor: POINT, right: bool, on_pick: PickFn, on_reorder: Option<ReorderFn>) {
    // A menu that is open gets its "closed" answer first.
    dismiss();
    create();
    let center = items.iter().any(|i| i.heading);
    with(|p| {
        p.on_pick = Some(on_pick);
        p.on_reorder = on_reorder;
        p.anchor = anchor;
        p.right = right;
        p.center = center;
    });
    let msg = app::with(|s| {
        let st = &s.settings;
        json!({
            "type": "show",
            "dark": st.dark,
            "accent": s.accent,
            "on_accent": st.on_accent,
            "colors": ui::colors_json(st),
            "radius": st.radius,
            "items": items_json(&items),
        })
        .to_string()
    });
    if let Some(m) = msg {
        send(m);
    }
}

/// New items for the open menu (e.g. after reordering a folder).
pub fn refresh(items: Vec<Item>) {
    send(json!({ "type": "items", "items": items_json(&items) }).to_string());
}

/// Closes the menu without a choice.
pub fn close() {
    dismiss();
}

fn dismiss() {
    let r = with(|p| {
        p.window.hide();
        p.on_reorder = None;
        p.on_pick.take()
    })
    .flatten();
    if let Some(f) = r {
        f(None);
    }
}

fn on_message(text: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return };
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    match v.get("cmd").and_then(|c| c.as_str()).unwrap_or("") {
        "size" => place(num("w"), num("h")),
        "pick" => {
            let id = v.get("id").and_then(|x| x.as_i64());
            let f = with(|p| {
                p.window.hide();
                p.on_reorder = None;
                p.on_pick.take()
            })
            .flatten();
            if let Some(f) = f {
                f(id);
            }
        }
        "close" => dismiss(),
        "reorder" => {
            let (from, to) = (num("from") as usize, num("to") as usize);
            call_reorder(from, to);
        }
        "drag_out" => call_reorder(num("from") as usize, DROPPED_OUTSIDE),
        _ => {}
    }
}

fn call_reorder(from: usize, to: usize) {
    let f = with(|p| p.on_reorder.take()).flatten();
    if let Some(mut f) = f {
        f(from, to);
        with(|p| {
            if p.on_reorder.is_none() && p.on_pick.is_some() {
                p.on_reorder = Some(f);
            }
        });
    }
}

/// The page reported its size (CSS px): size the window and put it in place.
fn place(wc: f64, hc: f64) {
    let z = app::with(|s| ui::zoom(&s.settings)).unwrap_or(1.0);
    let Some((window, view, anchor, right, center, open)) =
        with(|p| (p.window.clone(), p.view.clone(), p.anchor, p.right, p.center, p.on_pick.is_some()))
    else {
        return;
    };
    if !open {
        return;
    }
    view.set_zoom_level(z);
    let (w, h) = ((wc * z).ceil() as i32, (hc * z).ceil() as i32);
    // Work area of the monitor under the anchor.
    let wa = gtk::gdk::Display::default()
        .and_then(|d| d.monitor_at_point(anchor.x, anchor.y))
        .map(|m| m.workarea())
        .map(|r| (r.x(), r.y(), r.width(), r.height()))
        .unwrap_or((0, 0, 1920, 1080));
    let (x, y) = if center {
        // Dialog: centred on the browser window.
        let main = app::with(|s| s.window.clone());
        let (mx, my, mw, mh) = main
            .map(|m| {
                let (x, y) = m.position();
                let (w, hh) = m.size();
                (x, y, w, hh)
            })
            .unwrap_or(wa);
        (mx + (mw - w) / 2, my + ((mh / 3) - h / 2).max(60))
    } else {
        let x = if right { anchor.x - w } else { anchor.x };
        let mut y = anchor.y;
        if y + h > wa.1 + wa.3 {
            y = if anchor.y - h >= wa.1 { anchor.y - h } else { wa.1 + wa.3 - h };
        }
        (x, y)
    };
    let x = x.clamp(wa.0, (wa.0 + wa.2 - w).max(wa.0));
    let y = y.clamp(wa.1, (wa.1 + wa.3 - h).max(wa.1));
    window.set_size_request(w, h);
    window.set_default_size(w, h);
    window.resize(w, h);
    window.move_(x, y);
    with(|p| p.opened_at = std::time::Instant::now());
    window.show();
    // Again once it is on screen: a hidden window keeps its last size otherwise.
    window.resize(w, h);
    window.move_(x, y);
    let r = app::with(|s| if s.settings.radius > 0 { ((s.settings.radius + 2) as f64 * z).round() as i32 } else { 0 }).unwrap_or(0);
    round_corners(&window, w, h, r);
    window.present();
    view.grab_focus();
}

/// Without a compositor windows can't be see-through, so the rounded corners
/// are cut out of the window instead (like a window region on Windows).
fn round_corners(window: &gtk::Window, w: i32, h: i32, r: i32) {
    use gtk::cairo::{RectangleInt, Region};
    let composited = WidgetExt::screen(window).map(|s| s.is_composited()).unwrap_or(false);
    if composited || r <= 0 {
        window.shape_combine_region(None);
        return;
    }
    let r = r.min(w / 2).min(h / 2);
    let region = Region::create_rectangle(&RectangleInt::new(0, r, w, h - 2 * r));
    for y in 0..r {
        let dy = r as f64 - y as f64 - 0.5;
        let dx = (r as f64 - (r as f64 * r as f64 - dy * dy).max(0.0).sqrt()).round() as i32;
        let _ = region.union_rectangle(&RectangleInt::new(dx, y, w - 2 * dx, 1));
        let _ = region.union_rectangle(&RectangleInt::new(dx, h - 1 - y, w - 2 * dx, 1));
    }
    window.shape_combine_region(Some(&region));
}

// ---- Dialogs --------------------------------------------------------------------------

/// A message in the themed dialog: first line = title, the rest = text.
pub fn message(text: &str) {
    let (title, body) = text.split_once("\n\n").or_else(|| text.split_once('\n')).unwrap_or((text, ""));
    let mut items = vec![Item::heading(title.trim())];
    if !body.trim().is_empty() {
        items.push(Item::text(body.trim()));
    }
    items.push(Item::sep());
    items.push(Item::new(1, "OK").hot(true));
    show("message", items, POINT::default(), false, |_| {});
}

/// Yes / no question in the themed dialog.
pub fn confirm(title: &str, text: &str, yes: &str, no: &str, on_answer: impl FnOnce(bool) + 'static) {
    let items = vec![Item::heading(title), Item::text(text), Item::new(2, no), Item::new(1, yes).hot(true)];
    show("confirm", items, POINT::default(), false, move |pick| on_answer(pick == Some(1)));
}
