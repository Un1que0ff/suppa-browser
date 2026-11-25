// The Settings tab (https://suppa.settings/ = scripts\settings.html).
// The page asks for the current values with {cmd:"get"}, changes one with
// {cmd:"set", key, value} and runs buttons with {cmd:"action", name}.
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::core::HSTRING;

use crate::app::{self, *};
use crate::settings::{self, SEARCH_ENGINES};
use crate::{ui, util, webview};

pub fn on_message(wv: &ICoreWebView2, json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
    match v.get("cmd").and_then(|c| c.as_str()).unwrap_or("") {
        "get" => send_state(wv),
        "set" => {
            let key = v.get("key").and_then(|k| k.as_str()).unwrap_or("");
            let value = v.get("value").cloned().unwrap_or(serde_json::Value::Null);
            set(key, &value);
            send_state(wv);
        }
        "action" => {
            let cmd = match v.get("name").and_then(|n| n.as_str()).unwrap_or("") {
                "clear_data" => CMD_CLEAR_DATA,
                "clear_passwords" => CMD_CLEAR_PASSWORDS,
                "open_scripts" => CMD_OPEN_SCRIPTS,
                "open_data" => CMD_OPEN_DATA_DIR,
                "update_lists" => CMD_UPDATE_LISTS,
                "update" => CMD_UPDATE,
                "log" => CMD_OPEN_LOG,
                "incognito" => CMD_NEW_INCOGNITO,
                "store" => CMD_EXT_OPEN_STORE,
                "make_default" => CMD_MAKE_DEFAULT,
                "backup" => CMD_BACKUP,
                "restore" => CMD_RESTORE,
                "import" => CMD_IMPORT_MENU,
                "load_unpacked" => CMD_EXT_LOAD_UNPACKED,
                _ => return,
            };
            // Posted, so message boxes never run inside this WebView event.
            util::post(app::hwnd(), WM_APP_COMMAND, cmd as usize, 0);
        }
        _ => {}
    }
}

/// Refreshes every open Settings tab (e.g. after the theme changed from the menu).
pub fn broadcast() {
    let wvs = app::with(|s| {
        s.tabs
            .iter()
            .filter(|t| util::host_of(&t.url) == crate::tabs::SETTINGS_HOST)
            .filter_map(|t| t.webview.clone())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for w in &wvs {
        send_state(w);
    }
}

fn send_state(wv: &ICoreWebView2) {
    let json = app::with(|s| {
        let st = &s.settings;
        serde_json::json!({
            "type": "settings",
            "home": st.home,
            "search": st.search,
            "engines": SEARCH_ENGINES.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            "dark": st.dark,
            "accent": st.accent,
            "radius": st.radius,
            "ui_scale": st.ui_scale,
            "on_accent": st.on_accent,
            "bar_pos": st.bar_pos,
            "tabs_pos": st.tabs_pos,
            "nav_pos": st.nav_pos,
            "saved_pos": st.saved_pos,
            "row_order": st.row_order,
            "theme": st.theme_name(),
            "tint": st.tint,
            "tint_amount": st.tint_amount,
            "tab_corners": st.tab_corners,
            "corner_shape": st.corner_shape,
            "icon_pack": st.icon_pack,
            "colors": ui::custom_colors_json(st),
            "palette": ui::colors_json(st),
            "windows_accent": ui::windows_accent(),
            "adblock": st.adblock,
            "cookies": st.auto_cookies,
            "passwords": st.save_passwords,
            "autofill": st.autofill,
            "saved_bar": st.show_saved,
            "is_default": crate::default_browser::is_default(),
            "restore": st.restore_session,
            "sleep": st.sleep_minutes,
            "discard": st.discard_minutes,
            "allow": st.allow,
            "extensions": s.extensions.iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            "version": env!("CARGO_PKG_VERSION"),
            "engine": s.browser_version,
            "scripts": crate::scripts::dir().display().to_string(),
            "data": util::data_dir().display().to_string(),
        })
        .to_string()
    });
    if let Some(json) = json {
        unsafe {
            let _ = wv.PostWebMessageAsJson(&HSTRING::from(json));
        }
    }
}

fn set(key: &str, v: &serde_json::Value) {
    let b = v.as_bool().unwrap_or(false);
    let n = v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
    let text = v.as_str().unwrap_or("").trim().to_string();
    let mut theme_changed = false;
    let mut autofill_changed = false;
    let mut adblock_changed = false;
    let mut cookies_changed = false;
    let mut layout_changed = false;
    app::with(|s| {
        let st = &mut s.settings;
        match key {
            "home" => {
                st.home = if text.is_empty() {
                    settings::DEFAULT_HOME.to_string()
                } else {
                    let u = util::normalize_input(&text, st.search_base());
                    if u.is_empty() { settings::DEFAULT_HOME.to_string() } else { u }
                }
            }
            "search" => st.search = (n as usize).min(SEARCH_ENGINES.len() - 1),
            "dark" => {
                st.set_dark(b);
                s.ui.dark = b;
                theme_changed = true;
            }
            "theme" => {
                st.set_theme(&text);
                s.ui.dark = st.dark;
                theme_changed = true;
            }
            "tint" => {
                st.tint = settings::valid_tint(&text);
                theme_changed = true;
            }
            "tint_amount" => {
                st.tint_amount = settings::clamp_row(n as i32, settings::TINT);
                theme_changed = true;
            }
            "tab_corners" => {
                st.tab_corners = settings::valid_tab_corners(&text);
                theme_changed = true;
            }
            "corner_shape" => {
                st.corner_shape = settings::valid_corner_shape(&text);
                theme_changed = true;
            }
            "icon_pack" => {
                st.icon_pack = settings::valid_icon_pack(&text);
                theme_changed = true;
            }
            // Layout: place of one row, value {row, pos}
            "row_pos" => {
                let row = v.get("row").and_then(|x| x.as_str()).unwrap_or("");
                let pos = v.get("pos").and_then(|x| x.as_str()).unwrap_or("");
                st.set_row_pos(row, pos);
                theme_changed = true;
                layout_changed = true;
            }
            "accent" => {
                st.accent = settings::valid_accent(&text);
                theme_changed = true;
            }
            "radius" => {
                st.radius = settings::clamp_row(n as i32, settings::RADIUS);
                theme_changed = true;
            }
            "bar_pos" => {
                st.set_bar_pos(&text);
                theme_changed = true;
                layout_changed = true;
            }
            "row_order" => {
                st.row_order = settings::valid_order(&text);
                theme_changed = true;
            }
            "on_accent" => {
                st.on_accent = settings::valid_on_accent(&text);
                theme_changed = true;
            }
            "ui_scale" => {
                st.ui_scale = settings::clamp_row(n as i32, settings::UI_SCALE);
                layout_changed = true;
            }
            // Pro settings -> Colors (for the current theme)
            "color" => {
                let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
                let hex = v.get("hex").and_then(|x| x.as_str());
                st.set_color(name, hex);
                theme_changed = true;
            }
            "colors_reset" => {
                st.reset_colors();
                theme_changed = true;
            }
            "adblock" => {
                st.adblock = b;
                adblock_changed = true;
            }
            "cookies" => {
                st.auto_cookies = b;
                cookies_changed = true;
            }
            "passwords" => {
                st.save_passwords = b;
                autofill_changed = true;
            }
            "autofill" => {
                st.autofill = b;
                autofill_changed = true;
            }
            "restore" => st.restore_session = b,
            "saved_bar" => {
                st.show_saved = b;
                theme_changed = true;
                layout_changed = true;
            }
            "sleep" => st.sleep_minutes = n.clamp(1, 600),
            "discard" => st.discard_minutes = n.min(1440),
            "allow_remove" => st.allow.retain(|a| *a != text),
            _ => return,
        }
        settings::save(&s.settings);
        if adblock_changed {
            ui::refresh_toolbar(s);
        }
    });
    if theme_changed {
        ui::push_theme();
    }
    if cookies_changed {
        webview::apply_cookie_setting();
    }
    if layout_changed {
        ui::layout();
    }
    if autofill_changed {
        let wvs = app::with(|s| s.tabs.iter().filter_map(|t| t.webview.clone()).collect::<Vec<_>>()).unwrap_or_default();
        for w in &wvs {
            webview::apply_autofill(w);
        }
    }
}

/// Shows / hides the saved-tabs bar (Settings, or right-click the bar).
pub fn set_saved_bar(on: bool) {
    set("saved_bar", &serde_json::Value::Bool(on));
    broadcast();
}
