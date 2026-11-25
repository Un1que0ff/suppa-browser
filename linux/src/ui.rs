// The bar (tabs, address bar, saved tabs): scripts/ui.html in a WebKit view,
// the same page as on Windows. It talks to Suppa with
// window.chrome.webview.postMessage(...) and gets messages through the
// "message" event; BRIDGE_JS provides that API on Linux.
use std::cell::Cell;

use gtk::prelude::*;
use crate::wk::*;
use serde_json::json;
use webkit2gtk::{UserContentManager, WebView};

use crate::app::{self, *};
use crate::popup::POINT;
use crate::{settings, tabs, util};

/// window.chrome.webview for Suppa's own pages (suppa://...), like WebView2 has.
/// Web pages don't get it (sites check window.chrome to detect Chrome).
pub const BRIDGE_JS: &str = r#"(function(){
if(location.protocol!=='suppa:'||(window.chrome&&window.chrome.webview))return;
var L=[];window.chrome=window.chrome||{};
window.chrome.webview={
 postMessage:function(m){window.webkit.messageHandlers.suppa.postMessage(typeof m==='string'?m:JSON.stringify(m));},
 addEventListener:function(t,f){if(t==='message')L.push(f);},
 removeEventListener:function(t,f){var i=L.indexOf(f);if(i>=0)L.splice(i,1);}
};
Object.defineProperty(window,'__suppaRecv',{value:function(d){for(var i=0;i<L.length;i++){try{L[i]({data:d});}catch(e){console.error(e);}}}});
})();"#;

/// The bar's view: created once per window.
pub fn create(ctx: &webkit2gtk::WebContext) -> WebView {
    let ucm = UserContentManager::new();
    ucm.register_script_message_handler("suppa");
    ucm.add_script(&webkit2gtk::UserScript::new(
        BRIDGE_JS,
        webkit2gtk::UserContentInjectedFrames::TopFrame,
        webkit2gtk::UserScriptInjectionTime::Start,
        &[],
        &[],
    ));
    let st = webkit2gtk::Settings::new();
    st.set_enable_developer_extras(true);
    let view = WebView::builder().web_context(ctx).user_content_manager(&ucm).settings(&st).build();
    ucm.connect_script_message_received(Some("suppa"), |_, msg| {
        let text = msg.js_value().map(|v| v.to_str().to_string()).unwrap_or_default();
        // Handled a moment later: menus and new windows never open inside this event.
        app::later(move || on_message(&text));
    });
    // The bar has its own menus.
    view.connect_context_menu(|_, _, _, _| true);
    view.load_uri(crate::scripts::UI_URL);
    view
}

fn bar() -> Option<WebView> {
    app::with(|s| s.bar_ready.then(|| s.bar.clone())).flatten()
}

/// Sends a message (JSON text) to the bar.
pub fn post(json: String) {
    if let Some(b) = bar() {
        tabs::run_js(&b, &format!("window.__suppaRecv({json})"));
    }
}

/// Height of the bar in CSS px (top/bottom).
fn bar_css(st: &settings::Settings) -> i32 {
    st.row_tabs + st.row_nav + if st.show_saved { st.row_saved } else { 0 }
}

pub fn zoom(st: &settings::Settings) -> f64 {
    st.ui_scale as f64 / 100.0
}

/// Puts the bar where Settings says (top / bottom / left / right) at its size.
pub fn layout() {
    app::with(|s| {
        let st = &s.settings;
        let z = zoom(st);
        let pos = st.bar_pos.as_str();
        let vertical = pos == "top" || pos == "bottom";
        s.root.set_orientation(if vertical { gtk::Orientation::Vertical } else { gtk::Orientation::Horizontal });
        let first = pos == "top" || pos == "left";
        s.root.reorder_child(&s.bar, if first { 0 } else { 1 });
        if vertical {
            s.bar.set_size_request(-1, (bar_css(st) as f64 * z).round() as i32);
        } else {
            s.bar.set_size_request((st.side_width as f64 * z).round() as i32, -1);
        }
        s.bar.set_zoom_level(z);
        s.bar.set_visible(!s.fullscreen);
    });
}

// ---- Coordinates ----------------------------------------------------------------------

/// Screen position of the bar's top-left corner.
fn bar_origin() -> Option<(i32, i32, f64)> {
    app::with(|s| {
        let gw = s.bar.window()?;
        let (_, ox, oy) = gw.origin();
        let a = s.bar.allocation();
        let (x, y) = if s.bar.has_window() { (ox, oy) } else { (ox + a.x(), oy + a.y()) };
        Some((x, y, zoom(&s.settings)))
    })
    .flatten()
}

/// Bar CSS px -> screen px.
pub fn bar_to_screen(x: f64, y: f64) -> POINT {
    match bar_origin() {
        Some((ox, oy, z)) => POINT { x: ox + (x * z) as i32, y: oy + (y * z) as i32 },
        None => POINT::default(),
    }
}

/// Screen px -> bar CSS px, if the point is over the bar.
pub fn screen_to_bar(pt: POINT) -> Option<(f64, f64)> {
    let (ox, oy, z) = bar_origin()?;
    let (w, h) = app::with(|s| (s.bar.allocated_width(), s.bar.allocated_height()))?;
    let (x, y) = (pt.x - ox, pt.y - oy);
    (x >= 0 && y >= 0 && x < w && y < h).then(|| (x as f64 / z, y as f64 / z))
}

thread_local! {
    static MENU_AT: Cell<POINT> = const { Cell::new(POINT { x: 0, y: 0 }) };
    static SYNC_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Screen position where the last requested menu should open.
pub fn menu_anchor() -> POINT {
    MENU_AT.with(|m| m.get())
}

/// Screen point under the ≡ button (right edge of the bar) for notices.
pub fn corner_anchor() -> POINT {
    let r = app::with(|s| (s.bar.allocated_width(), s.settings.row_tabs + s.settings.row_nav - 4, zoom(&s.settings)));
    let Some((w, below, z)) = r else { return POINT::default() };
    bar_to_screen(w as f64 / z - 12.0, below as f64)
}

// ---- Focus ----------------------------------------------------------------------------

pub fn focus_bar() {
    if let Some(b) = app::with(|s| s.bar.clone()) {
        b.grab_focus();
    }
}

pub fn focus_webview() {
    if let Some(v) = tabs::active_view() {
        v.grab_focus();
    }
}

pub fn focus_address() {
    focus_bar();
    post(json!({ "type": "focus_address" }).to_string());
}

// ---- Messages from the bar ----------------------------------------------------------------

fn command_for(name: &str) -> Option<u16> {
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

fn on_message(json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
    let s_str = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    match v.get("cmd").and_then(|c| c.as_str()).unwrap_or("") {
        "ready" => {
            let first = app::with(|s| !std::mem::replace(&mut s.bar_ready, true)).unwrap_or(false);
            if first {
                util::log(&format!("startup: bar ready at {} ms", util::since_start_ms()));
            }
            push_theme();
            layout();
            sync_now();
            post_update();
            crate::saved::sync();
            post(json!({ "type": "exts", "items": [] }).to_string());
        }
        "drag" => crate::window::begin_drag(&s_str("edge")),
        "navigate" => tabs::navigate_active(&s_str("text")),
        "activate" => tabs::activate(num("index") as usize),
        "close" => tabs::close_tab(num("index") as usize),
        "run" => {
            if let Some(cmd) = command_for(&s_str("name")) {
                crate::window::command(cmd);
            }
        }
        "star" => crate::saved::toggle_active(),
        "move_tab" => tabs::move_tab(num("from") as usize, num("to") as usize),
        "tab_group" => crate::groups::group_with(num("from") as usize, num("target") as usize),
        "group_toggle" => crate::groups::toggle(num("id") as u32),
        "group_menu" => crate::groups::menu(num("id") as u32, bar_to_screen(num("x"), num("y"))),
        "group_rename" => crate::groups::rename(num("id") as u32, &s_str("name")),
        "tab_detach" => tabs::detach(num("index") as usize),
        "rows" => {
            use crate::settings::{ROW_NAV, ROW_SAVED, ROW_TABS, clamp_row};
            let save = v.get("save").and_then(|b| b.as_bool()).unwrap_or(false);
            app::with(|s| {
                if s.settings.lock_sizes {
                    return;
                }
                s.settings.row_tabs = clamp_row(num("tabs") as i32, ROW_TABS);
                s.settings.row_nav = clamp_row(num("nav") as i32, ROW_NAV);
                s.settings.row_saved = clamp_row(num("saved") as i32, ROW_SAVED);
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
            app::with(|s| s.accent = c);
        }
        "tab_menu" => crate::menu::show_tab(num("index") as usize, bar_to_screen(num("x"), num("y"))),
        c @ ("saved_open" | "saved_folder" | "saved_menu" | "saved_move" | "saved_rename") => {
            let pt = bar_to_screen(num("x"), num("y"));
            crate::saved::on_message(c, &v, pt);
        }
        "menu" => {
            MENU_AT.with(|m| m.set(bar_to_screen(num("x"), num("y"))));
            match s_str("which").as_str() {
                "adblock" => crate::menu::show_adblock(),
                "extensions" => crate::popup::message("Extensions\nChrome extensions are not available in the Linux version (WebKitGTK does not support them)."),
                _ => crate::menu::show_main(),
            }
        }
        "set_theme" => {
            let dark = v.get("dark").and_then(|d| d.as_bool()).unwrap_or(true);
            app::with(|s| {
                if s.settings.dark != dark {
                    s.settings.dark = dark;
                    settings::save(&s.settings);
                }
            });
            crate::window::apply_theme(dark, &s_str("bg"));
        }
        "focus_page" => focus_webview(),
        _ => {}
    }
}

/// Sends theme + accent colour to the bar (it answers with set_theme).
pub fn push_theme() {
    let r = app::with(|s| {
        let st = &s.settings;
        json!({
            "type": "init", "dark": st.dark, "accent": accent_for_bar(st), "saved": st.show_saved,
            "rows": [st.row_tabs, st.row_nav, st.row_saved], "locked": st.lock_sizes, "radius": st.radius,
            "colors": colors_json(st),
            "on_accent": st.on_accent,
            "order": st.row_order, "bar_pos": st.bar_pos, "side_width": st.side_width,
            // Linux: one bar panel (all rows at bar_pos), window buttons stay in it.
            "native_buttons": false,
            "squircle": st.corner_shape == "squircle", "icons": st.icon_pack, "tab_corners": st.tab_corners,
        })
        .to_string()
    });
    if let Some(json) = r {
        post(json);
    }
}

/// The desktop's accent colour ("windows" accent setting): GNOME's when set, else blue.
pub fn system_accent() -> String {
    let out = std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "accent-color"]).output();
    let name = out.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_string()).unwrap_or_default();
    match name.as_str() {
        "teal" => "#2190a4",
        "green" => "#3a944a",
        "yellow" => "#c88800",
        "orange" => "#ed5b00",
        "red" => "#e62d42",
        "pink" => "#d56199",
        "purple" => "#9141ac",
        "slate" => "#6f8396",
        _ => "#3584e4",
    }
    .to_string()
}

/// Accent setting as the bar understands it: "cycle" or a colour.
pub fn accent_for_bar(st: &settings::Settings) -> String {
    if st.accent == "windows" { system_accent() } else { st.accent.clone() }
}

/// Custom colours of the current theme as {"bg": "#101010", ...}.
pub fn colors_json(st: &settings::Settings) -> serde_json::Value {
    serde_json::Value::Object(st.colors().iter().map(|(n, h)| (n.clone(), serde_json::Value::String(h.clone()))).collect())
}

/// Update state of the ≡ button ("Update ≡" / "Building… ≡").
pub fn post_update() {
    if let Some((avail, building)) = app::with(|s| (s.update_available, s.building)) {
        post(format!("{{\"type\":\"update\",\"available\":{avail},\"building\":{building}}}"));
    }
}

/// Sends tabs + address bar state to the bar (at most once per main-loop turn).
pub fn request_sync() {
    if !SYNC_PENDING.with(|p| p.replace(true)) {
        app::later(sync_now);
    }
}

pub fn sync_now() {
    SYNC_PENDING.with(|p| p.set(false));
    let json = app::with(|s| {
        let list: Vec<serde_json::Value> = s
            .tabs
            .iter()
            .map(|t| {
                let state = if t.loading {
                    "loading"
                } else if t.view.is_none() && !tabs::is_newtab(&t.url) {
                    "unloaded"
                } else {
                    ""
                };
                let fav = if t.incognito || !t.favicon.starts_with("http") { "" } else { t.favicon.as_str() };
                json!({ "id": t.id, "t": tab_title(t), "s": state, "i": t.incognito, "f": fav, "g": t.group })
            })
            .collect();
        let t = s.active_tab();
        let url = t.map(|t| if tabs::is_newtab(&t.url) { String::new() } else { t.url.clone() }).unwrap_or_default();
        let host = util::host_of(&url);
        let ads = if !s.settings.adblock {
            "off"
        } else if s.settings.is_allowed(&host) {
            "site"
        } else {
            "on"
        };
        let view = t.and_then(|t| t.view.clone());
        let back = view.as_ref().map(|v| v.can_go_back()).unwrap_or(false);
        let fwd = view.as_ref().map(|v| v.can_go_forward()).unwrap_or(false);
        let title = match t {
            Some(t) if !t.title.is_empty() => format!("{} - Suppa", t.title),
            _ => "Suppa".to_string(),
        };
        s.window.set_title(&title);
        json!({
            "type": "sync",
            "tabs": list,
            "active": s.active,
            "url": url,
            "blocked": 0,
            "ads": ads,
            "back": back,
            "fwd": fwd,
            "max": s.maximized,
            "inc": t.map(|t| t.incognito).unwrap_or(false),
            "groups": crate::groups::json(s),
            "star": !url.is_empty() && crate::saved::contains(&url),
        })
        .to_string()
    });
    if let Some(json) = json {
        post(json);
    }
}

pub fn tab_title(t: &app::Tab) -> String {
    if !t.title.is_empty() {
        t.title.clone()
    } else if tabs::is_newtab(&t.url) {
        "New Tab".to_string()
    } else {
        util::host_of(&t.url)
    }
}

pub fn set_update_available(on: bool) {
    let changed = app::with(|s| std::mem::replace(&mut s.update_available, on) != on).unwrap_or(false);
    if changed {
        if on {
            util::log("scripts folder / Rust code changed - Update button lit");
        }
        post_update();
    }
}
