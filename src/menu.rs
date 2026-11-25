// Menus: the "≡" main menu (Update, Extensions, Ad blocker, Log, Theme,
// Settings...), the ad-blocker menu and the tab right-click menu. They are drawn
// by the themed popup (popup.rs + scripts\menu.html).
use windows::Win32::Foundation::POINT;

use crate::app::{self, *};
use crate::popup::{self, Item};
use crate::{tabs, util};

/// Runs a chosen command id.
fn run(pick: Option<i64>) {
    if let Some(id) = pick {
        if id > 0 {
            crate::window::command(id as u16);
        }
    }
}

pub fn show_main() {
    let Some((closed, update, building, dark, blocked, ext_count, locked, show_saved, dl_button)) = app::with(|s| {
        (
            !s.closed.is_empty(),
            s.ui.update_available,
            s.building,
            s.settings.dark,
            s.active_tab().map(|t| t.blocked).unwrap_or(0),
            s.extensions.len(),
            s.settings.lock_sizes,
            s.settings.show_saved,
            s.settings.dl_button,
        )
    }) else {
        return;
    };
    let upd = if building {
        "Building new version..."
    } else if update {
        "Update  ●  changes ready"
    } else {
        "Update (restart and reload scripts)"
    };
    let ext_label = if ext_count > 0 { format!("Extensions ({ext_count})") } else { "Extensions".to_string() };
    let items = vec![
        Item::cmd(CMD_UPDATE, upd).enabled(!building).hot(update && !building),
        Item::sep(),
        Item::cmd(CMD_NEW_TAB, "New tab").key("Ctrl+T"),
        Item::cmd(CMD_NEW_INCOGNITO, "New incognito tab").key("Ctrl+Shift+N"),
        Item::cmd(CMD_REOPEN_TAB, "Reopen closed tab").key("Ctrl+Shift+T").enabled(closed),
        Item::sep(),
        Item::sub(ext_label, crate::extensions::menu_items()),
        Item::cmd(CMD_EXT_OPEN_STORE, "Chrome Web Store (extensions)"),
        Item::sub(format!("Ad blocker - {blocked} blocked"), adblock_items()),
        Item::sub(
            "Page",
            vec![
                Item::cmd(CMD_ZOOM_IN, "Zoom in").key("Ctrl++"),
                Item::cmd(CMD_ZOOM_OUT, "Zoom out").key("Ctrl+-"),
                Item::cmd(CMD_ZOOM_RESET, "Reset zoom").key("Ctrl+0"),
                Item::sep(),
                Item::cmd(CMD_PRINT, "Print...").key("Ctrl+P"),
                Item::cmd(CMD_DEVTOOLS, "Developer tools").key("F12"),
            ],
        ),
        Item::cmd(CMD_FULLSCREEN, "Full screen").key("F11"),
        Item::cmd(CMD_HISTORY, "History").key("Ctrl+H"),
        Item::cmd(CMD_DOWNLOADS, "Downloads").key("Ctrl+J"),
        Item::cmd(CMD_TASK_MANAGER, "Task manager").key("Shift+Esc"),
        Item::cmd(CMD_SLEEP_NOW, "Sleep background tabs now"),
        Item::sep(),
        Item::cmd(CMD_TOGGLE_THEME, if dark { "Light theme" } else { "Dark theme" }),
        Item::cmd(CMD_LOCK_SIZES, "Lock bar sizes").checked(locked),
        Item::new(SHOW_SAVED_ID, "Show saved tabs bar").checked(show_saved),
        Item::new(SHOW_DL_ID, "Show download button").checked(dl_button),
        Item::cmd(CMD_SETTINGS, "Settings").key("Alt+S"),
        Item::cmd(CMD_CHECK_UPDATE, "Check for updates"),
        Item::sub("Backup and import", crate::import::menu_items()),
        Item::cmd(CMD_OPEN_LOG, "Log"),
        Item::cmd(CMD_OPEN_SCRIPTS, "Open scripts folder"),
        Item::cmd(CMD_ABOUT, "About Suppa"),
        Item::sep(),
        Item::cmd(CMD_CLOSE_WINDOW, "Exit"),
    ];
    popup::show("main", items, crate::ui::menu_anchor(), true, move |pick| {
        if pick == Some(SHOW_SAVED_ID) {
            crate::settings_page::set_saved_bar(!show_saved);
        } else if pick == Some(SHOW_DL_ID) {
            app::with(|s| {
                s.settings.dl_button = !dl_button;
                crate::settings::save(&s.settings);
            });
            crate::downloads::sync_bar();
        } else if let Some(id) = pick.filter(|&id| crate::import::run(id)) {
            let _ = id;
        } else {
            run(pick);
        }
    });
}

const SHOW_SAVED_ID: i64 = 60_000;
const SHOW_DL_ID: i64 = 60_001;

pub fn show_adblock() {
    popup::show("adblock", adblock_items(), crate::ui::menu_anchor(), true, run);
}

/// The ad-blocker menu (also a submenu of ≡).
pub fn adblock_items() -> Vec<Item> {
    let r = app::with(|s| {
        let t = s.active_tab();
        let host = t.map(|t| util::host_of(&t.doc_url)).unwrap_or_default();
        let blocked = t.map(|t| t.blocked).unwrap_or(0);
        (s.settings.adblock, s.settings.is_allowed(&host), host, blocked)
    });
    let Some((on, site_off, host, blocked)) = r else { return Vec::new() };
    let status = if !crate::adblock::is_ready() {
        "Filter lists loading...".to_string()
    } else {
        format!("{blocked} ads/trackers blocked on this page")
    };
    let mut items = vec![
        Item::new(0, status).enabled(false),
        Item::sep(),
        Item::cmd(CMD_TOGGLE_ADBLOCK, "Ad blocker enabled").checked(on),
    ];
    if !host.is_empty() {
        items.push(Item::cmd(CMD_TOGGLE_SITE, format!("Allow ads on {host}")).checked(site_off));
    }
    items.push(Item::sep());
    items.push(Item::cmd(CMD_UPDATE_LISTS, "Update filter lists now"));
    items.push(Item::cmd(CMD_OPEN_SCRIPTS, "Edit my filter rules (scripts\\filters)"));
    items
}

/// Right-click on a tab.
pub fn show_tab(index: usize, anchor: POINT) {
    let Some((saved, closed, n, inc, grouped)) = app::with(|s| {
        let t = s.tabs.get(index);
        (
            t.map(|t| crate::saved::contains(&t.url)).unwrap_or(false),
            !s.closed.is_empty(),
            s.tabs.len(),
            t.map(|t| t.incognito).unwrap_or(false),
            t.map(|t| t.group != 0).unwrap_or(false),
        )
    }) else {
        return;
    };
    let items = vec![
        Item::new(1, if saved { "Saved ✓" } else { "Save" }).enabled(!saved && !inc),
        Item::sep(),
        Item::new(2, "New tab").key("Ctrl+T"),
        Item::new(3, "Reload").key("F5"),
        Item::new(4, "Duplicate"),
        Item::sep(),
        Item::new(5, "Close tab").key("Ctrl+W"),
        Item::new(6, "Close other tabs").enabled(n > 1),
        Item::new(7, "Reopen closed tab").key("Ctrl+Shift+T").enabled(closed),
        Item::sep(),
        if grouped { Item::new(9, "Remove from group") } else { Item::new(8, "Add to new group") },
        Item::new(10, "Ungroup (whole group)").enabled(grouped),
    ];
    popup::show("tab", items, anchor, false, move |pick| match pick {
        Some(1) => crate::saved::save_tab(index),
        Some(2) => {
            tabs::new_tab(None, true);
        }
        Some(3) => tabs::reload_index(index),
        Some(4) => tabs::duplicate_index(index),
        Some(5) => tabs::close_tab(index),
        Some(6) => tabs::close_others_except(index),
        Some(7) => tabs::reopen_closed(),
        Some(8) => crate::groups::add_to_new_group(index),
        Some(9) => crate::groups::remove_from_group(index),
        Some(10) => crate::groups::ungroup_tab(index),
        _ => {}
    });
}
