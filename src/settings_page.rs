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
                "import_passwords" => CMD_IMPORT_PASSWORDS,
                "export_passwords" => CMD_EXPORT_PASSWORDS,
                "restart" => CMD_RESTART,
                "check_update" => CMD_CHECK_UPDATE,
                "whats_new" => CMD_WHATS_NEW,
                "history" => CMD_HISTORY,
                "downloads" => CMD_DOWNLOADS,
                "load_unpacked" => CMD_EXT_LOAD_UNPACKED,
                "shortcuts" => CMD_SHORTCUTS,
                "sync_now" => CMD_SYNC_NOW,
                "sync_folder" => CMD_SYNC_FOLDER,
                _ => return,
            };
            // Posted, so message boxes never run inside this WebView event.
            util::post(app::hwnd(), WM_APP_COMMAND, cmd as usize, 0);
        }
        // Addresses, cards, PayPal, Suppa's passwords, generator, Dropbox sync.
        "wallet" => crate::vault::on_settings_message(wv, &v),
        // Password manager: {cmd:"pw", op, realm, user}
        "pw" => {
            let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let (realm, user) = (s("realm"), s("user"));
            let reply = match s("op").as_str() {
                "list" => crate::passwords::list_json(),
                // Showing or copying needs Windows Hello / the account password first.
                op @ ("reveal" | "copy") => {
                    let wv = wv.clone();
                    let reason = if op == "reveal" { "Suppa wants to show a saved password" } else { "Suppa wants to copy a saved password" };
                    let op = op.to_string();
                    crate::auth::require(reason, move |ok| {
                        let reply = if !ok {
                            serde_json::json!({ "type": "pw_toast", "text": "Not confirmed - the password stays hidden" })
                        } else if op == "reveal" {
                            serde_json::json!({
                                "type": "pw_reveal", "realm": realm, "user": user,
                                "password": crate::passwords::reveal(&realm, &user),
                            })
                        } else {
                            let ok = crate::passwords::copy(&realm, &user);
                            serde_json::json!({ "type": "pw_toast", "text": if ok { "Password copied" } else { "Could not copy the password" } })
                        };
                        unsafe {
                            let _ = wv.PostWebMessageAsJson(&HSTRING::from(reply.to_string()));
                        }
                    });
                    return;
                }
                "delete" => {
                    crate::passwords::delete(&realm, &user);
                    crate::passwords::list_json()
                }
                "undo" => {
                    crate::passwords::undo_deletes();
                    crate::passwords::list_json()
                }
                _ => return,
            };
            unsafe {
                let _ = wv.PostWebMessageAsJson(&HSTRING::from(reply.to_string()));
            }
        }
        _ => {}
    }
}

/// Refreshes every open Settings tab (e.g. after the theme changed from the menu).
pub fn broadcast() {
    let wvs = app::with(|s| {
        s.tabs
            .iter()
            // Settings, and the History / Downloads pages (same theme message)
            .filter(|t| {
                let h = util::host_of(&t.url);
                h == crate::tabs::SETTINGS_HOST || crate::pages::is_page(&h)
            })
            .filter_map(|t| t.webview.clone())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for w in &wvs {
        send_state(w);
    }
}

pub fn send_state(wv: &ICoreWebView2) {
    let json = app::with(|s| {
        let st = &s.settings;
        let mut j = serde_json::json!({
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
            "auto_update": st.auto_update,
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
        });
        // Added separately (one json! this long hits the macro's recursion limit).
        if let Some(m) = j.as_object_mut() {
            m.insert("cookie_mode".into(), st.cookie_mode.clone().into());
            if st.theme_system {
                m.insert("theme".into(), "system".into());
            }
            m.insert("fill_forms".into(), st.fill_forms.into());
            m.insert("offer_save".into(), st.offer_save.into());
            m.insert("sync_on".into(), st.sync_on.into());
            m.insert("player_button".into(), st.player_button.into());
            m.insert("dl_button".into(), st.dl_button.into());
        }
        j.to_string()
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
    let mut sync_changed = false;
    let mut bar_buttons = false;
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
                st.theme_system = false;
                st.set_dark(b);
                s.ui.dark = b;
                theme_changed = true;
            }
            "theme" if text == "system" => {
                st.theme_system = true;
                st.set_dark(!ui::windows_light());
                s.ui.dark = st.dark;
                theme_changed = true;
            }
            "theme" => {
                st.theme_system = false;
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
            "cookie_mode" => {
                st.cookie_mode = if text == "all" { "all" } else { "necessary" }.to_string();
                cookies_changed = true;
            }
            "fill_forms" => st.fill_forms = b,
            "offer_save" => st.offer_save = b,
            "sync_on" => {
                st.sync_on = b;
                sync_changed = true;
            }
            "player_button" => {
                st.player_button = b;
                bar_buttons = true;
            }
            "dl_button" => {
                st.dl_button = b;
                bar_buttons = true;
            }
            "restore" => st.restore_session = b,
            "auto_update" => st.auto_update = b,
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
    if bar_buttons {
        crate::media::sync_bar();
        crate::downloads::sync_bar();
    }
    if sync_changed {
        crate::sync::sync_now();
        refresh_wallet();
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

/// Open Settings pages get the wallet lists and sync status again.
pub fn refresh_wallet() {
    let wvs = app::with(|s| {
        s.tabs
            .iter()
            .filter(|t| util::host_of(&t.url) == crate::tabs::SETTINGS_HOST)
            .filter_map(|t| t.webview.clone())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    if wvs.is_empty() {
        return;
    }
    let json = crate::vault::list_json().to_string();
    for w in &wvs {
        unsafe {
            let _ = w.PostWebMessageAsJson(&HSTRING::from(json.clone()));
        }
    }
}
