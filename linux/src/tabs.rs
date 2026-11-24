// Tabs: open / close / switch / move / navigate, and the WebKit view of each tab.
use std::time::Instant;

use gtk::prelude::*;
use crate::wk::*;
use webkit2gtk::{
    LoadEvent, NavigationPolicyDecision, PolicyDecisionType, ResponsePolicyDecision, UserContentManager, WebView};

use crate::app::{self, Tab};
use crate::{adblock, settings, ui, util};

pub const SETTINGS_URL: &str = "suppa://settings/";
pub const NEWTAB_URL: &str = "suppa://newtab/";

pub fn is_newtab(url: &str) -> bool {
    url.is_empty() || url.starts_with(NEWTAB_URL) || url == "about:blank"
}

/// URL Suppa shows for a tab: Windows-style names of Suppa's own pages are
/// mapped to the suppa:// pages.
fn fix_url(url: &str) -> String {
    let base = app::with(|s| s.settings.search_base()).unwrap_or(settings::SEARCH_ENGINES[0].1);
    let u = util::normalize_input(url, base);
    if u.is_empty() { NEWTAB_URL.to_string() } else { u }
}

// ---- WebKit views -------------------------------------------------------------------

/// Scripts every page gets: the chrome.webview bridge (for Suppa's own pages),
/// cookie-banner clicker, ad-block helper and your user scripts.
pub fn add_scripts(ucm: &UserContentManager, incognito: bool) {
    use webkit2gtk::{UserContentInjectedFrames, UserScript, UserScriptInjectionTime};
    ucm.remove_all_scripts();
    let add = |code: &str, all_frames: bool, start: bool| {
        let frames = if all_frames { UserContentInjectedFrames::AllFrames } else { UserContentInjectedFrames::TopFrame };
        let when = if start { UserScriptInjectionTime::Start } else { UserScriptInjectionTime::End };
        ucm.add_script(&UserScript::new(code, frames, when, &[], &[]));
    };
    add(ui::BRIDGE_JS, false, true);
    let (cookies, block) = app::with(|s| (s.settings.auto_cookies, s.settings.adblock)).unwrap_or((true, true));
    if block {
        add(adblock::COLLECTOR_SCRIPT, false, true);
    }
    if cookies {
        add(crate::scripts::cookie_script(), true, false);
    }
    let _ = incognito; // private tabs run your user scripts too, like on Windows
    for code in crate::scripts::user_scripts() {
        add(&code, false, false);
    }
}

/// Settings -> Accept cookie popups changed: re-applied to every open tab.
pub fn apply_scripts_setting() {
    let views = app::with(|s| s.tabs.iter().filter_map(|t| t.view.clone().map(|v| (v, t.incognito))).collect::<Vec<_>>())
        .unwrap_or_default();
    for (v, inc) in views {
        if let Some(ucm) = v.user_content_manager() {
            add_scripts(&ucm, inc);
        }
    }
}

fn web_settings() -> webkit2gtk::Settings {
    let st = webkit2gtk::Settings::new();
    st.set_enable_developer_extras(true);
    st.set_javascript_can_open_windows_automatically(false);
    st.set_enable_smooth_scrolling(true);
    st.set_enable_back_forward_navigation_gestures(true);
    st.set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::OnDemand);
    st
}

/// Makes the WebKit view of tab `id` (not yet shown). `related`: a page that
/// opened this one (window.open), so they can talk to each other.
fn make_view(id: u32, incognito: bool, related: Option<&WebView>) -> Option<WebView> {
    let ctx = if incognito { crate::window::private_context() } else { app::with(|s| s.ctx.clone())? };
    let ucm = UserContentManager::new();
    ucm.register_script_message_handler("suppa");
    add_scripts(&ucm, incognito);
    adblock::attach(&ucm, true);
    let mut b = WebView::builder().user_content_manager(&ucm).settings(&web_settings());
    b = match related {
        Some(r) => b.related_view(r),
        None => b.web_context(&ctx),
    };
    let view = b.build();
    let bg = app::with(|s| if s.settings.dark { "#161616" } else { "#f2f2f2" }).unwrap_or("#161616");
    if let Ok(c) = bg.parse::<gtk::gdk::RGBA>() {
        view.set_background_color(&c);
    }
    connect(&view, id, &ucm);
    Some(view)
}

fn connect(view: &WebView, id: u32, ucm: &UserContentManager) {
    view.connect_title_notify(move |v| {
        let title = v.title().map(|t| t.to_string()).unwrap_or_default();
        app::with(|s| {
            if let Some(t) = s.tab_mut(id) {
                t.title = title;
            }
        });
        ui::request_sync();
    });
    view.connect_uri_notify(move |v| {
        let uri = v.uri().map(|u| u.to_string()).unwrap_or_default();
        app::with(|s| {
            if let Some(t) = s.tab_mut(id) {
                t.url = uri;
            }
        });
        ui::request_sync();
        crate::window::save_session_soon();
    });
    view.connect_load_changed(move |v, ev| {
        let uri = v.uri().map(|u| u.to_string()).unwrap_or_default();
        match ev {
            LoadEvent::Started => {
                app::with(|s| {
                    if let Some(t) = s.tab_mut(id) {
                        t.loading = true;
                        t.favicon.clear();
                    }
                });
                // "Allow ads on this site": no blocking rules for that page.
                if let Some(ucm) = v.user_content_manager() {
                    let on = app::with(|s| s.settings.adblock && !s.settings.is_allowed(&util::host_of(&uri))).unwrap_or(true);
                    adblock::attach(&ucm, on);
                }
            }
            LoadEvent::Committed => {
                // Site-specific hiding rules and anti-ad scriptlets.
                let on = app::with(|s| s.settings.adblock && !s.settings.is_allowed(&util::host_of(&uri))).unwrap_or(false);
                if on {
                    let js = adblock::document_script(&uri);
                    if !js.is_empty() {
                        run_js(v, &js);
                    }
                }
            }
            LoadEvent::Finished => {
                app::with(|s| {
                    if let Some(t) = s.tab_mut(id) {
                        t.loading = false;
                    }
                });
                // Favicon URL for the tab (the bar loads it itself).
                if util::is_web_url(&uri) {
                    run_js(v, "window.webkit.messageHandlers.suppa.postMessage(JSON.stringify({t:'suppa-icon',u:(function(){var l=document.querySelector('link[rel~=\"icon\"]');return l&&l.href?l.href:location.origin+'/favicon.ico';})()}));");
                }
                crate::window::save_session_soon();
            }
            _ => {}
        }
        ui::request_sync();
    });
    view.connect_create(move |v, _action| {
        // target=_blank / window.open: a new tab next to this one.
        let inc = app::with(|s| s.tabs.iter().find(|t| t.id == id).map(|t| t.incognito)).flatten().unwrap_or(false);
        let new_id = app::with(|s| {
            let nid = s.next_id;
            s.next_id += 1;
            nid
        })?;
        let nv = make_view(new_id, inc, Some(v))?;
        let w: gtk::Widget = nv.clone().upcast();
        nv.connect_ready_to_show(move |_| {
            if let Some(i) = app::with(|s| s.tab_index(new_id)).flatten() {
                activate(i);
            }
        });
        app::with(|s| {
            let at = s.tab_index(id).map(|i| i + 1).unwrap_or(s.tabs.len());
            let mut t = Tab::new(new_id, String::new());
            t.incognito = inc;
            t.view = Some(nv.clone());
            s.tabs.insert(at.min(s.tabs.len()), t);
            if at <= s.active {
                s.active += 1;
            }
            s.pages.add_named(&nv, &new_id.to_string());
            crate::groups::inherit(s, at);
        });
        nv.show();
        ui::request_sync();
        Some(w)
    });
    view.connect_decide_policy(move |_v, decision, kind| {
        match kind {
            PolicyDecisionType::NewWindowAction => {
                // Ctrl/middle click on a link: background tab.
                if let Some(nav) = decision.downcast_ref::<NavigationPolicyDecision>() {
                    if let Some(action) = nav.navigation_action() {
                        let bg = action.mouse_button() == 2 || action.modifiers() & 4 != 0; // GDK_CONTROL_MASK
                        if bg {
                            if let Some(req) = action.request() {
                                if let Some(u) = req.uri() {
                                    new_tab(Some(u.to_string()), false);
                                    decision.ignore();
                                    return true;
                                }
                            }
                        }
                    }
                }
                false
            }
            PolicyDecisionType::Response => {
                // Files the engine cannot show are downloaded.
                if let Some(r) = decision.downcast_ref::<ResponsePolicyDecision>() {
                    if !r.is_mime_type_supported() && r.is_main_frame_main_resource() {
                        decision.download();
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    });
    view.connect_close(move |_| {
        if let Some(i) = app::with(|s| s.tab_index(id)).flatten() {
            close_tab(i);
        }
    });
    view.connect_enter_fullscreen(|_| {
        crate::window::set_fullscreen(true);
        false
    });
    view.connect_leave_fullscreen(|_| {
        crate::window::set_fullscreen(false);
        false
    });
    view.connect_web_process_terminated(move |v, _| {
        util::log("a page process ended unexpectedly - reloading the tab");
        v.reload();
    });
    let weak = view.downgrade();
    ucm.connect_script_message_received(Some("suppa"), move |_, msg| {
        let Some(view) = weak.upgrade() else { return };
        let text = msg.js_value().map(|v| v.to_str().to_string()).unwrap_or_default();
        on_page_message(&view, id, &text);
    });
}

/// Runs JavaScript in a page (result ignored).
pub fn run_js(view: &WebView, js: &str) {
    view.evaluate_javascript(js, None, None, None::<&gtk::gio::Cancellable>, |_| {});
}

/// Messages from pages: Suppa's settings page, favicons, ad-block class lists.
fn on_page_message(view: &WebView, id: u32, text: &str) {
    let uri = view.uri().map(|u| u.to_string()).unwrap_or_default();
    if uri.starts_with(SETTINGS_URL) {
        crate::settings_page::on_message(view, text);
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return };
    match v.get("t").and_then(|t| t.as_str()) {
        Some("suppa-icon") => {
            let u = v.get("u").and_then(|u| u.as_str()).unwrap_or("").to_string();
            if u.starts_with("http") {
                app::with(|s| {
                    if let Some(t) = s.tab_mut(id) {
                        t.favicon = u;
                    }
                });
                ui::request_sync();
            }
        }
        Some("suppa-cos") => {
            let on = app::with(|s| s.settings.adblock && !s.settings.is_allowed(&util::host_of(&uri))).unwrap_or(false);
            if on {
                let js = adblock::generic_hide_script(&uri, text);
                if !js.is_empty() {
                    run_js(view, &js);
                }
            }
        }
        _ => {}
    }
}

// ---- Tab operations ---------------------------------------------------------------------

/// Creates the view of tab `idx` if it was unloaded (or never loaded).
fn ensure_view(idx: usize) {
    let Some(Some((id, inc, url))) =
        app::with(|s| s.tabs.get(idx).filter(|t| t.view.is_none()).map(|t| (t.id, t.incognito, t.url.clone())))
    else {
        return;
    };
    let Some(view) = make_view(id, inc, None) else { return };
    app::with(|s| {
        if let Some(t) = s.tab_mut(id) {
            t.view = Some(view.clone());
        }
        s.pages.add_named(&view, &id.to_string());
    });
    view.show();
    view.load_uri(&fix_url(&url));
}

/// Opens a tab (url None = home page). `select`: switch to it.
pub fn new_tab(url: Option<String>, select: bool) -> Option<usize> {
    add_tab(url, select, false)
}

pub fn new_incognito_url(url: String) {
    add_tab(Some(url), true, true);
}

pub fn new_incognito() {
    add_tab(None, true, true);
}

fn add_tab(url: Option<String>, select: bool, incognito: bool) -> Option<usize> {
    // Ctrl+T / + button: the cursor goes to the address bar.
    let focus_address = select && url.is_none();
    let url = url.unwrap_or_else(|| app::with(|s| s.settings.home.clone()).unwrap_or_default());
    let idx = app::with(|s| {
        let id = s.next_id;
        s.next_id += 1;
        let mut t = Tab::new(id, url);
        t.incognito = incognito;
        t.focus_address = focus_address;
        // New tabs go after the current one's group / right after it.
        let at = if s.tabs.is_empty() { 0 } else { s.tabs.len() };
        s.tabs.insert(at, t);
        at
    })?;
    ensure_view(idx);
    if select {
        activate(idx);
    }
    ui::request_sync();
    crate::window::save_session_soon();
    Some(idx)
}

/// Adds tabs from the saved session without loading them (only the active one loads).
pub fn restore(urls: Vec<String>, active: usize) {
    app::with(|s| {
        for u in urls {
            let id = s.next_id;
            s.next_id += 1;
            s.tabs.push(Tab::new(id, u));
        }
    });
    activate(active);
}

pub fn activate(idx: usize) {
    let ok = app::with(|s| idx < s.tabs.len()).unwrap_or(false);
    if !ok {
        return;
    }
    ensure_view(idx);
    let r = app::with(|s| {
        s.active = idx;
        let t = &mut s.tabs[idx];
        t.last_active = Instant::now();
        let focus_addr = std::mem::take(&mut t.focus_address);
        // A selected tab is never inside a collapsed group.
        let g = t.group;
        if let Some(gr) = s.groups.iter_mut().find(|x| x.id == g) {
            gr.collapsed = false;
        }
        (s.pages.clone(), s.tabs[idx].id, s.tabs[idx].view.clone(), focus_addr)
    });
    let Some((pages, id, view, focus_addr)) = r else { return };
    pages.set_visible_child_name(&id.to_string());
    if focus_addr || view.as_ref().map(|v| is_newtab(&v.uri().map(|u| u.to_string()).unwrap_or_default())).unwrap_or(false) {
        ui::focus_address();
    } else if let Some(v) = view {
        v.grab_focus();
    }
    ui::request_sync();
    crate::window::save_session_soon();
}

pub fn close_tab(idx: usize) {
    let r = app::with(|s| {
        if idx >= s.tabs.len() {
            return None;
        }
        let t = s.tabs.remove(idx);
        if !t.incognito && !t.url.is_empty() {
            s.closed.push((t.url.clone(), idx));
            if s.closed.len() > 30 {
                s.closed.remove(0);
            }
        }
        if let Some(v) = &t.view {
            s.pages.remove(v);
        }
        crate::groups::cleanup(s);
        let empty = s.tabs.is_empty();
        if !empty {
            if s.active > idx || s.active >= s.tabs.len() {
                s.active = s.active.saturating_sub(1);
            }
            s.active = s.active.min(s.tabs.len() - 1);
        }
        if !s.tabs.iter().any(|t| t.incognito) {
            s.private_ctx = None; // forget everything of the private tabs
        }
        Some((empty, s.active, t.view))
    });
    let Some(Some((empty, active, view))) = r else { return };
    drop(view);
    if empty {
        crate::window::close_window();
        return;
    }
    activate(active);
}

pub fn close_active() {
    if let Some(i) = app::with(|s| s.active) {
        close_tab(i);
    }
}

pub fn close_others_except(idx: usize) {
    let keep = app::with(|s| s.tabs.get(idx).map(|t| t.id)).flatten();
    let Some(keep) = keep else { return };
    loop {
        let next = app::with(|s| s.tabs.iter().position(|t| t.id != keep)).flatten();
        match next {
            Some(i) => close_tab(i),
            None => break,
        }
    }
}

pub fn reopen_closed() {
    if let Some(Some((url, pos))) = app::with(|s| s.closed.pop()) {
        if let Some(i) = new_tab(Some(url), false) {
            let to = pos.min(app::with(|s| s.tabs.len().saturating_sub(1)).unwrap_or(0));
            move_tab(i, to);
            activate(to);
        }
    }
}

pub fn move_tab(from: usize, to: usize) {
    app::with(|s| {
        if from >= s.tabs.len() || to >= s.tabs.len() || from == to {
            return;
        }
        let active_id = s.tabs[s.active].id;
        let t = s.tabs.remove(from);
        s.tabs.insert(to, t);
        if let Some(i) = s.tab_index(active_id) {
            s.active = i;
        }
        crate::groups::after_move(s, to);
    });
    ui::request_sync();
    crate::window::save_session_soon();
}

/// Dragged a tab out of the window: it opens in a new Suppa window.
pub fn detach(idx: usize) {
    let r = app::with(|s| (s.tabs.len() > 1).then(|| s.tabs.get(idx).map(|t| (t.url.clone(), t.incognito))).flatten());
    let Some(Some((url, inc))) = r else { return };
    let mut cmd = std::process::Command::new(crate::scripts::exe_path());
    cmd.arg("--window");
    if inc {
        cmd.arg("--incognito");
    }
    cmd.arg(url);
    if cmd.spawn().is_ok() {
        close_tab(idx);
    }
}

pub fn navigate_active(text: &str) {
    let url = fix_url(text);
    let r = app::with(|s| {
        let a = s.active;
        s.tabs.get_mut(a).map(|t| {
            t.url = url.clone();
            a
        })
    });
    let Some(Some(idx)) = r else { return };
    ensure_view(idx);
    if let Some(Some(v)) = app::with(|s| s.tabs.get(idx).and_then(|t| t.view.clone())) {
        v.load_uri(&url);
        v.grab_focus();
    }
}

pub fn active_view() -> Option<WebView> {
    app::with(|s| s.active_tab().and_then(|t| t.view.clone())).flatten()
}

pub fn reload_index(idx: usize) {
    if let Some(Some(v)) = app::with(|s| s.tabs.get(idx).and_then(|t| t.view.clone())) {
        v.reload();
    }
}

pub fn duplicate_index(idx: usize) {
    let r = app::with(|s| s.tabs.get(idx).map(|t| (t.url.clone(), t.incognito))).flatten();
    if let Some((url, inc)) = r {
        if inc {
            new_incognito_url(url);
        } else if let Some(i) = new_tab(Some(url), true) {
            move_tab(i, idx + 1);
        }
    }
}

pub fn select_relative(d: i32) {
    let r = app::with(|s| {
        let n = s.tabs.len() as i32;
        (n > 0).then(|| ((s.active as i32 + d).rem_euclid(n)) as usize)
    });
    if let Some(Some(i)) = r {
        activate(i);
    }
}

/// Background tabs not used for a while are unloaded to free memory
/// (Settings -> Unload background tabs after). They load again when selected.
pub fn unload_idle() {
    let minutes = app::with(|s| s.settings.discard_minutes).unwrap_or(0);
    if minutes == 0 {
        return;
    }
    let views = app::with(|s| {
        let active = s.active;
        let mut out = Vec::new();
        for (i, t) in s.tabs.iter_mut().enumerate() {
            let idle = t.last_active.elapsed().as_secs() >= minutes * 60;
            if i != active && idle && !t.incognito && !t.loading {
                if let Some(v) = t.view.take() {
                    out.push(v);
                }
            }
        }
        for v in &out {
            s.pages.remove(v);
        }
        out
    })
    .unwrap_or_default();
    if !views.is_empty() {
        util::log(&format!("unloaded {} background tab(s)", views.len()));
        ui::request_sync();
    }
}

/// All background tabs unloaded now (≡ -> Sleep background tabs now).
pub fn unload_all_background() {
    let views = app::with(|s| {
        let active = s.active;
        let mut out = Vec::new();
        for (i, t) in s.tabs.iter_mut().enumerate() {
            if i != active && !t.incognito {
                if let Some(v) = t.view.take() {
                    out.push(v);
                }
            }
        }
        for v in &out {
            s.pages.remove(v);
        }
        out
    })
    .unwrap_or_default();
    drop(views);
    ui::request_sync();
}
