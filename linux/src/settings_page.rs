// The Settings tab (suppa://settings/ = scripts/settings.html, the same page as
// on Windows). The page asks for the current values with {cmd:"get"}, changes
// one with {cmd:"set", key, value} and runs buttons with {cmd:"action", name}.
use webkit2gtk::WebView;

use crate::app::{self, *};
use crate::settings::{self, SEARCH_ENGINES};
use crate::{tabs, ui, util};

pub fn on_message(view: &WebView, json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
    match v.get("cmd").and_then(|c| c.as_str()).unwrap_or("") {
        "get" => send_state(view),
        "set" => {
            let key = v.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string();
            let value = v.get("value").cloned().unwrap_or(serde_json::Value::Null);
            set(&key, &value);
            send_state(view);
        }
        "action" => {
            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let cmd = match name.as_str() {
                "clear_data" => CMD_CLEAR_DATA,
                "open_scripts" => CMD_OPEN_SCRIPTS,
                "open_data" => CMD_OPEN_DATA_DIR,
                "update_lists" => CMD_UPDATE_LISTS,
                "update" => CMD_UPDATE,
                "log" => CMD_OPEN_LOG,
                "incognito" => CMD_NEW_INCOGNITO,
                "make_default" => CMD_MAKE_DEFAULT,
                "clear_passwords" | "store" | "backup" | "restore" | "import" | "load_unpacked" => {
                    app::later(move || {
                        crate::popup::message(&format!(
                            "Not in the Linux version yet\n\"{}\" is only available in Suppa for Windows for now.",
                            match name.as_str() {
                                "clear_passwords" => "Saved passwords",
                                "store" | "load_unpacked" => "Extensions",
                                "import" => "Import bookmarks",
                                _ => "Backup and restore",
                            }
                        ))
                    });
                    return;
                }
                _ => return,
            };
            app::post_command(cmd);
        }
        _ => {}
    }
}

/// Refreshes every open Settings tab (e.g. after the theme changed from the menu).
pub fn broadcast() {
    let views = app::with(|s| {
        s.tabs
            .iter()
            .filter(|t| t.url.starts_with(tabs::SETTINGS_URL))
            .filter_map(|t| t.view.clone())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for v in &views {
        send_state(v);
    }
}

fn send_state(view: &WebView) {
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
            // One bar panel on Linux: every row is where the bar is.
            "tabs_pos": st.bar_pos,
            "nav_pos": st.bar_pos,
            "saved_pos": st.bar_pos,
            "theme": st.theme_name(),
            "corner_shape": st.corner_shape,
            "icon_pack": st.icon_pack,
            "tab_corners": st.tab_corners,
            "row_order": st.row_order,
            "colors": ui::colors_json(st),
            "windows_accent": ui::system_accent(),
            "adblock": st.adblock,
            "cookies": st.auto_cookies,
            "passwords": st.save_passwords,
            "autofill": st.autofill,
            "saved_bar": st.show_saved,
            "is_default": crate::window::is_default_browser(),
            "restore": st.restore_session,
            "sleep": st.sleep_minutes,
            "discard": st.discard_minutes,
            "allow": st.allow,
            "extensions": Vec::<String>::new(),
            "version": env!("CARGO_PKG_VERSION"),
            "platform": "linux",
            "engine": crate::window::engine_version(),
            "scripts": crate::scripts::dir().display().to_string(),
            "data": util::data_dir().display().to_string(),
        })
        .to_string()
    });
    if let Some(json) = json {
        tabs::run_js(view, &format!("window.__suppaRecv({json})"));
    }
}

fn set(key: &str, v: &serde_json::Value) {
    let b = v.as_bool().unwrap_or(false);
    let n = v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
    let text = v.as_str().unwrap_or("").trim().to_string();
    let mut theme_changed = false;
    let mut scripts_changed = false;
    let mut layout_changed = false;
    let mut adblock_changed = false;
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
                theme_changed = true;
            }
            "theme" => {
                st.set_theme(&text);
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
            "tab_corners" => {
                st.tab_corners = settings::valid_tab_corners(&text);
                theme_changed = true;
            }
            // Per-row places are Windows-only; on Linux the whole bar moves.
            "row_pos" => {
                st.set_bar_pos(v.get("pos").and_then(|x| x.as_str()).unwrap_or("top"));
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
                scripts_changed = true;
            }
            "cookies" => {
                st.auto_cookies = b;
                scripts_changed = true;
            }
            "passwords" => st.save_passwords = b,
            "autofill" => st.autofill = b,
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
    });
    if theme_changed {
        ui::push_theme();
    }
    if scripts_changed {
        tabs::apply_scripts_setting();
    }
    if adblock_changed {
        crate::adblock::reattach_all();
        ui::request_sync();
    }
    if layout_changed {
        ui::layout();
    }
}

/// Shows / hides the saved-tabs bar (Settings, or right-click the bar).
pub fn set_saved_bar(on: bool) {
    set("saved_bar", &serde_json::Value::Bool(on));
    broadcast();
}
