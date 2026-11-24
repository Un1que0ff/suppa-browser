// Shared application state. Everything runs on the GTK main thread, so the
// state lives in a thread_local RefCell. Use `with(|s| ...)` for short borrows;
// it returns None instead of panicking when the state is already borrowed.
use std::cell::RefCell;
use std::time::Instant;

use webkit2gtk::{WebContext, WebView};

use crate::settings::Settings;

// ---- Command IDs (buttons, menus, shortcuts) - same numbers as on Windows ----------
pub const CMD_UPDATE: u16 = 90;
pub const CMD_OPEN_LOG: u16 = 91;
pub const CMD_OPEN_SCRIPTS: u16 = 92;
pub const CMD_SETTINGS: u16 = 93;
pub const CMD_NEW_INCOGNITO: u16 = 94;
pub const CMD_TOGGLE_THEME: u16 = 95;
pub const CMD_MINIMIZE: u16 = 96;
pub const CMD_MAXIMIZE: u16 = 97;
pub const CMD_CLOSE_WINDOW: u16 = 98;
pub const CMD_SAVED_RENAME: u16 = 133;
pub const CMD_MAKE_DEFAULT: u16 = 134;
pub const CMD_LOCK_SIZES: u16 = 135;

pub const CMD_NEW_TAB: u16 = 100;
pub const CMD_CLOSE_TAB: u16 = 101;
pub const CMD_REOPEN_TAB: u16 = 102;
pub const CMD_NEXT_TAB: u16 = 103;
pub const CMD_PREV_TAB: u16 = 104;
pub const CMD_BACK: u16 = 105;
pub const CMD_FORWARD: u16 = 106;
pub const CMD_RELOAD: u16 = 107;
pub const CMD_HOME: u16 = 108;
pub const CMD_FOCUS_ADDRESS: u16 = 109;
pub const CMD_FULLSCREEN: u16 = 110;
pub const CMD_ESCAPE: u16 = 111;
pub const CMD_MENU: u16 = 112;
pub const CMD_ADBLOCK_MENU: u16 = 113;
pub const CMD_DOWNLOADS: u16 = 115;
pub const CMD_SLEEP_NOW: u16 = 117;
pub const CMD_CLEAR_DATA: u16 = 118;
pub const CMD_ABOUT: u16 = 119;
pub const CMD_TOGGLE_ADBLOCK: u16 = 120;
pub const CMD_TOGGLE_SITE: u16 = 121;
pub const CMD_UPDATE_LISTS: u16 = 122;
pub const CMD_DEVTOOLS: u16 = 123;
pub const CMD_ZOOM_IN: u16 = 124;
pub const CMD_ZOOM_OUT: u16 = 125;
pub const CMD_ZOOM_RESET: u16 = 126;
pub const CMD_PRINT: u16 = 127;
pub const CMD_OPEN_DATA_DIR: u16 = 128;
pub const CMD_HARD_RELOAD: u16 = 129;
pub const CMD_CLOSE_OTHER_TABS: u16 = 131;
pub const CMD_DUPLICATE_TAB: u16 = 132;
pub const CMD_TAB_BASE: u16 = 200; // +0..8  (Ctrl+1..Ctrl+9)

// ---- Data ---------------------------------------------------------------------
pub struct Tab {
    pub id: u32,
    /// None = unloaded (to save memory); created again when the tab is selected.
    pub view: Option<WebView>,
    /// Last committed URL (shown in the address bar).
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub last_active: Instant,
    /// Private tab: nothing saved, not in the session.
    pub incognito: bool,
    pub favicon: String,
    /// Put the cursor in the address bar once this tab's page exists (Ctrl+T).
    pub focus_address: bool,
    /// Tab group id (0 = not in a group).
    pub group: u32,
}

impl Tab {
    pub fn new(id: u32, url: String) -> Self {
        Tab {
            id,
            view: None,
            url,
            title: String::new(),
            loading: false,
            last_active: Instant::now(),
            incognito: false,
            favicon: String::new(),
            focus_address: false,
            group: 0,
        }
    }
}

/// A group of tabs (label + colour in the tab row).
pub struct Group {
    pub id: u32,
    pub name: String,
    pub color: String,
    pub collapsed: bool,
}

pub struct State {
    pub window: gtk::Window,
    /// Holds the bar and the page area; its orientation follows the bar position.
    pub root: gtk::Box,
    pub bar: WebView,
    pub bar_ready: bool,
    /// Page area: one child per loaded tab.
    pub pages: gtk::Stack,
    pub ctx: WebContext,
    /// Shared by all private tabs of this window (dropped when they are closed).
    pub private_ctx: Option<WebContext>,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub next_id: u32,
    /// Recently closed tabs: (url, position) for Ctrl+Shift+T.
    pub closed: Vec<(String, usize)>,
    /// A Rust rebuild (Update button) is running.
    pub building: bool,
    pub update_available: bool,
    pub groups: Vec<Group>,
    pub next_group: u32,
    /// Extra window opened by dragging a tab out: no session restore/save.
    pub secondary: bool,
    pub settings: Settings,
    pub fullscreen: bool,
    pub maximized: bool,
    /// Current accent colour shown by the bar (menus use it too).
    pub accent: String,
}

impl State {
    pub fn tab_index(&self, id: u32) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }
    pub fn tab_mut(&mut self, id: u32) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn init(state: State) {
    STATE.with(|c| *c.borrow_mut() = Some(state));
}

/// Borrow the global state. Returns None if not initialised or already borrowed
/// (re-entrancy), so callers never panic.
pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|c| match c.try_borrow_mut() {
        Ok(mut g) => g.as_mut().map(f),
        Err(_) => None,
    })
}

/// Runs a command a moment later, outside the current event (like PostMessage on Windows).
pub fn post_command(cmd: u16) {
    gtk::glib::idle_add_local_once(move || crate::window::command(cmd));
}

/// Runs `f` a moment later, outside the current event.
pub fn later(f: impl FnOnce() + 'static) {
    gtk::glib::idle_add_local_once(f);
}
