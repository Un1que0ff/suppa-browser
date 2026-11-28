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
    let Some((closed, update, building, dark, blocked, ext_count, locked, show_saved, dl_button, player_button, awake, muted)) = app::with(|s| {
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
            s.settings.player_button,
            s.active_tab().map(|t| t.awake).unwrap_or(false),
            s.active_tab().map(|t| t.muted).unwrap_or(false),
        )
    }) else {
        return;
    };
    let upd = if building {
        &crate::i18n::tr("Building new version...")
    } else if update {
        &crate::i18n::tr("Update  ●  changes ready")
    } else {
        &crate::i18n::tr("Restart and reload scripts")
    };
    let ext_label = if ext_count > 0 { crate::i18n::trf("Extensions ({})", &[&ext_count.to_string()]) } else { crate::i18n::tr("Extensions") };
    // Grouped so the everyday things are on top and every setting is one
    // click away: tabs, your stuff, this page, add-ons, look, settings, help.
    let mut items = Vec::new();
    if update || building {
        items.push(Item::cmd(CMD_UPDATE, upd).enabled(!building).hot(update && !building));
        items.push(Item::sep());
    }
    items.extend([
        Item::cmd(CMD_NEW_TAB, crate::i18n::tr("New tab")).key("Ctrl+T"),
        Item::cmd(CMD_NEW_INCOGNITO, crate::i18n::tr("New incognito tab")).key("Ctrl+Shift+N"),
        Item::cmd(CMD_REOPEN_TAB, crate::i18n::tr("Reopen closed tab")).key("Ctrl+Shift+T").enabled(closed),
        Item::sep(),
        Item::cmd(CMD_HISTORY, crate::i18n::tr("History")).key("Ctrl+H"),
        Item::cmd(CMD_DOWNLOADS, crate::i18n::tr("Downloads")).key("Ctrl+J"),
        Item::cmd(CMD_PASSWORDS, crate::i18n::tr("Passwords")),
        Item::cmd(CMD_WALLET, crate::i18n::tr("Addresses and payments")),
        Item::cmd(CMD_PLAYER, crate::i18n::tr("Player")).key("Ctrl+Shift+M"),
        Item::sep(),
        Item::sub(
            crate::i18n::tr("This page"),
            vec![
                Item::cmd(CMD_ZOOM_IN, crate::i18n::tr("Zoom in")).key("Ctrl++"),
                Item::cmd(CMD_ZOOM_OUT, crate::i18n::tr("Zoom out")).key("Ctrl+-"),
                Item::cmd(CMD_ZOOM_RESET, crate::i18n::tr("Reset zoom")).key("Ctrl+0"),
                Item::sep(),
                Item::cmd(CMD_KEEP_AWAKE, crate::i18n::tr("Keep this tab awake (never sleeps)")).checked(awake),
                Item::cmd(CMD_MUTE_TAB, if muted { crate::i18n::tr("Unmute this tab") } else { crate::i18n::tr("Mute this tab") }).key("Ctrl+M"),
                Item::sep(),
                Item::cmd(CMD_SAVE_VIDEO, crate::i18n::tr("Save video")),
                Item::cmd(CMD_SAVE_MP3, crate::i18n::tr("Save as MP3")),
                Item::sep(),
                Item::cmd(CMD_STAR, crate::i18n::tr("Save to saved tabs")).key("Ctrl+D"),
                Item::cmd(CMD_PRINT, crate::i18n::tr("Print...")).key("Ctrl+P"),
                Item::cmd(CMD_VIEW_SOURCE, crate::i18n::tr("Page source")).key("Ctrl+U"),
                Item::cmd(CMD_DEVTOOLS, crate::i18n::tr("Developer tools")).key("F12"),
            ],
        ),
        Item::cmd(CMD_FULLSCREEN, crate::i18n::tr("Full screen")).key("F11"),
        Item::sep(),
        Item::sub(crate::i18n::trf("Ad blocker - {} blocked", &[&blocked.to_string()]), adblock_items()),
        Item::sub(ext_label, crate::extensions::menu_items()),
        Item::cmd(CMD_EXT_OPEN_STORE, crate::i18n::tr("Get extensions (Chrome Web Store)")),
        Item::sep(),
        Item::sub(
            crate::i18n::tr("Look and toolbar"),
            vec![
                Item::cmd(CMD_TOGGLE_THEME, if dark { crate::i18n::tr("Switch to light theme") } else { crate::i18n::tr("Switch to dark theme") }),
                Item::sep(),
                Item::new(SHOW_SAVED_ID, crate::i18n::tr("Show saved tabs bar")).checked(show_saved).key("Ctrl+Shift+B"),
                Item::new(SHOW_DL_ID, crate::i18n::tr("Show download button")).checked(dl_button),
                Item::cmd(CMD_TOGGLE_PLAYER_BTN, crate::i18n::tr("Show player button")).checked(player_button),
                Item::cmd(CMD_LOCK_SIZES, crate::i18n::tr("Lock bar sizes")).checked(locked),
                Item::sep(),
                Item::cmd(CMD_SETTINGS, crate::i18n::tr("More in Settings...")),
            ],
        ),
        Item::cmd(CMD_SETTINGS, crate::i18n::tr("Settings")).key("Alt+S"),
        Item::sub(crate::i18n::tr("Backup and import"), crate::import::menu_items()),
        Item::sub(
            crate::i18n::tr("Help and updates"),
            vec![
                Item::cmd(CMD_SHORTCUTS, crate::i18n::tr("Keyboard shortcuts")).key("F1"),
                Item::cmd(CMD_CHECK_UPDATE, crate::i18n::tr("Check for updates")),
                Item::cmd(CMD_WHATS_NEW, crate::i18n::tr("What's new")),
                Item::cmd(CMD_ABOUT, crate::i18n::tr("About Suppa")),
            ],
        ),
        Item::sub(
            crate::i18n::tr("Advanced"),
            vec![
                Item::cmd(CMD_UPDATE, upd).enabled(!building).hot(update && !building),
                Item::cmd(CMD_TASK_MANAGER, crate::i18n::tr("Task manager")).key("Shift+Esc"),
                Item::cmd(CMD_SLEEP_NOW, crate::i18n::tr("Sleep background tabs now")),
                Item::cmd(CMD_GEN_PASSWORD, crate::i18n::tr("Copy a strong password")).key("Ctrl+Shift+G"),
                Item::sep(),
                Item::cmd(CMD_OPEN_LOG, crate::i18n::tr("Log")),
                Item::cmd(CMD_OPEN_SCRIPTS, crate::i18n::tr("Open scripts folder")),
                Item::cmd(CMD_OPEN_DATA_DIR, crate::i18n::tr("Open data folder")),
            ],
        ),
        Item::sep(),
        Item::cmd(CMD_CLOSE_WINDOW, crate::i18n::tr("Exit")),
    ]);
    popup::show("main", items, crate::ui::menu_anchor(), true, move |pick| {
        if pick == Some(SHOW_SAVED_ID) {
            crate::settings_page::set_saved_bar(!show_saved);
        } else if pick == Some(SHOW_DL_ID) {
            app::with(|s| {
                s.settings.dl_button = !dl_button;
                crate::settings::save(&s.settings);
            });
            crate::downloads::sync_bar();
            crate::settings_page::broadcast();
        } else if let Some(id) = pick.filter(|&id| crate::import::run(id)) {
            let _ = id;
        } else {
            run(pick);
        }
    });
}

/// ≡ -> Help -> Keyboard shortcuts (F1).
pub fn show_shortcuts() {
    let mut items = vec![Item::heading(crate::i18n::tr("Keyboard shortcuts"))];
    let text: Vec<String> = crate::shortcuts::SHORTCUTS.iter().map(|(k, d)| format!("{k}  -  {d}")).collect();
    items.push(Item::text(text.join("\n")));
    items.push(Item::sep());
    items.push(Item::new(1, crate::i18n::tr("OK")).hot(true));
    popup::show("dialog-shortcuts", items, POINT::default(), false, |_| {});
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
        crate::i18n::tr("Filter lists loading...")
    } else {
        crate::i18n::trf("{} ads/trackers blocked on this page", &[&blocked.to_string()])
    };
    let mut items = vec![
        Item::new(0, status).enabled(false),
        Item::sep(),
        Item::cmd(CMD_TOGGLE_ADBLOCK, crate::i18n::tr("Ad blocker enabled")).checked(on),
    ];
    if !host.is_empty() {
        items.push(Item::cmd(CMD_TOGGLE_SITE, crate::i18n::trf("Allow ads on {}", &[&host])).checked(site_off));
    }
    items.push(Item::sep());
    items.push(Item::cmd(CMD_UPDATE_LISTS, crate::i18n::tr("Update filter lists now")));
    items.push(Item::cmd(CMD_OPEN_SCRIPTS, crate::i18n::tr("Edit my filter rules (scripts\\filters)")));
    items
}

/// Right-click on a tab.
pub fn show_tab(index: usize, anchor: POINT) {
    let Some((saved, closed, n, inc, grouped, awake)) = app::with(|s| {
        let t = s.tabs.get(index);
        (
            t.map(|t| crate::saved::contains(&t.url)).unwrap_or(false),
            !s.closed.is_empty(),
            s.tabs.len(),
            t.map(|t| t.incognito).unwrap_or(false),
            t.map(|t| t.group != 0).unwrap_or(false),
            t.map(|t| t.awake).unwrap_or(false),
        )
    }) else {
        return;
    };
    let items = vec![
        Item::new(1, if saved { crate::i18n::tr("Saved ✓") } else { crate::i18n::tr("Save") }).enabled(!saved && !inc),
        Item::sep(),
        Item::new(2, crate::i18n::tr("New tab")).key("Ctrl+T"),
        Item::new(3, crate::i18n::tr("Reload")).key("F5"),
        Item::new(4, crate::i18n::tr("Duplicate")),
        Item::new(11, crate::i18n::tr("Keep awake (never sleeps in the background)")).checked(awake),
        Item::sub(crate::i18n::tr("Sound"), crate::media::volume_items(index)),
        Item::sep(),
        Item::new(5, crate::i18n::tr("Close tab")).key("Ctrl+W"),
        Item::new(6, crate::i18n::tr("Close other tabs")).enabled(n > 1),
        Item::new(7, crate::i18n::tr("Reopen closed tab")).key("Ctrl+Shift+T").enabled(closed),
        Item::sep(),
        if grouped { Item::new(9, crate::i18n::tr("Remove from group")) } else { Item::new(8, crate::i18n::tr("Add to new group")) },
        Item::new(10, crate::i18n::tr("Ungroup (whole group)")).enabled(grouped),
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
        Some(11) => tabs::toggle_awake(index),
        Some(p) if p >= crate::media::VOL_MUTE => crate::media::volume_pick(index, p),
        _ => {}
    });
}
