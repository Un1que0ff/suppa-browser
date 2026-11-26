// Shared application state. Everything runs on the single UI thread, so the
// state lives in a thread_local RefCell. Use `with(|s| ...)` for short borrows and
// never call modal functions (menus, message boxes, SetWindowPos on the main
// window) while inside `with`.
use std::cell::RefCell;
use std::time::Instant;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{WINDOWPLACEMENT, WM_APP};

use crate::settings::Settings;
use crate::ui::Ui;

// ---- Private window messages -------------------------------------------------
pub const WM_APP_ADBLOCK_READY: u32 = WM_APP + 1;
pub const WM_APP_EXT_UNPACKED: u32 = WM_APP + 2;
pub const WM_APP_CLOSE_TAB_ID: u32 = WM_APP + 3;
pub const WM_APP_FULLSCREEN: u32 = WM_APP + 4;
pub const WM_APP_COMMAND: u32 = WM_APP + 5;
pub const WM_APP_UI_SYNC: u32 = WM_APP + 6;
pub const WM_APP_CLOSE_TAB_INDEX: u32 = WM_APP + 7;
/// Window drag / resize started from the HTML bar (wParam = hit-test code).
pub const WM_APP_NC_DRAG: u32 = WM_APP + 8;
/// Rust rebuild finished (wParam 1 = success).
pub const WM_APP_BUILD_DONE: u32 = WM_APP + 9;
/// A file changed in the scripts folder / Rust source / exe folder.
pub const WM_APP_FILES_CHANGED: u32 = WM_APP + 10;
/// Runs the closures queued with later.
pub const WM_APP_LATER: u32 = WM_APP + 11;

pub const TIMER_SLEEP: usize = 1;
/// Checks the scripts folder for changes (lights up the Update button).
pub const TIMER_WATCH: usize = 2;
/// Saves the open tabs shortly after they change (so a crash loses nothing).
pub const TIMER_SESSION: usize = 3;

// ---- Command IDs (buttons, menus, shortcuts) ----------------------------------
pub const CMD_UPDATE: u16 = 90;
pub const CMD_OPEN_LOG: u16 = 91;
pub const CMD_OPEN_SCRIPTS: u16 = 92;
pub const CMD_SETTINGS: u16 = 93;
pub const CMD_NEW_INCOGNITO: u16 = 94;
pub const CMD_TOGGLE_THEME: u16 = 95;
pub const CMD_MINIMIZE: u16 = 96;
pub const CMD_MAXIMIZE: u16 = 97;
pub const CMD_CLOSE_WINDOW: u16 = 98;
pub const CMD_CLEAR_PASSWORDS: u16 = 99;
pub const CMD_SAVED_RENAME: u16 = 133;
pub const CMD_MAKE_DEFAULT: u16 = 134;
pub const CMD_LOCK_SIZES: u16 = 135;
pub const CMD_BACKUP: u16 = 136;
pub const CMD_RESTORE: u16 = 137;
pub const CMD_IMPORT_MENU: u16 = 138;
pub const CMD_IMPORT_PASSWORDS: u16 = 139;
pub const CMD_EXPORT_PASSWORDS: u16 = 140;
pub const CMD_RESTART: u16 = 141;
pub const CMD_AUTH_OK: u16 = 142;
pub const CMD_AUTH_FAIL: u16 = 143;
pub const CMD_HISTORY: u16 = 144;
pub const CMD_DL_POPUP: u16 = 145;
pub const CMD_CHECK_UPDATE: u16 = 146;
pub const CMD_INSTALL_UPDATE: u16 = 147;
pub const CMD_WHATS_NEW: u16 = 148;
pub const CMD_DL_CHANGED: u16 = 149;
pub const CMD_UPDATE_FOUND: u16 = 150;
pub const CMD_NOTES_READY: u16 = 151;
pub const CMD_DL_POLL: u16 = 152;

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
pub const CMD_EXT_MENU: u16 = 114;
pub const CMD_DOWNLOADS: u16 = 115;
pub const CMD_TASK_MANAGER: u16 = 116;
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
pub const CMD_TOGGLE_RESTORE: u16 = 130;
pub const CMD_CLOSE_OTHER_TABS: u16 = 131;
pub const CMD_DUPLICATE_TAB: u16 = 132;
pub const CMD_SEARCH_BASE: u16 = 150; // +index of search engine
pub const CMD_TAB_BASE: u16 = 200; // +0..8  (Ctrl+1..Ctrl+9)

pub const CMD_EXT_INSTALL_STORE: u16 = 2901;
pub const CMD_EXT_LOAD_UNPACKED: u16 = 2902;
pub const CMD_EXT_OPEN_STORE: u16 = 2903;
pub const CMD_EXT_OPEN_FOLDER: u16 = 2904;
pub const CMD_EXT_BASE: u16 = 3000; // +index*8 + action

// ---- Data ---------------------------------------------------------------------
pub struct Tab {
    pub id: u32,
    pub controller: Option<ICoreWebView2Controller>,
    pub webview: Option<ICoreWebView2>,
    /// Last committed URL (shown in the address bar).
    pub url: String,
    /// URL of the document currently loading/loaded (used as ad-block "source").
    pub doc_url: String,
    pub title: String,
    pub loading: bool,
    pub blocked: u32,
    pub last_active: Instant,
    pub suspended: bool,
    pub creating: bool,
    /// Per-navigation ad-block script (cosmetic CSS + scriptlets) registration id.
    pub doc_script_id: Option<String>,
    /// A popup / target=_blank request waiting for this tab's WebView to exist.
    pub pending_new_window: Option<(ICoreWebView2NewWindowRequestedEventArgs, ICoreWebView2Deferral)>,
    /// InPrivate tab: separate profile, nothing saved, not in the session.
    pub incognito: bool,
    pub favicon: String,
    /// Put the cursor in the address bar once this tab's page exists (Ctrl+T).
    pub focus_address: bool,
    /// Registration id of the cookie-banner clicker in this tab.
    pub cookie_script_id: Option<String>,
    /// Tab group id (0 = not in a group).
    pub group: u32,
    /// The last address written to the history from this tab (the first page
    /// of a tab opened with an address counts too, though `url` already has it).
    pub recorded: String,
}

/// A group of tabs (label + colour in the tab row).
pub struct Group {
    pub id: u32,
    pub name: String,
    pub color: String,
    pub collapsed: bool,
}

impl Tab {
    pub fn new(id: u32, url: String) -> Self {
        Tab {
            incognito: false,
            favicon: String::new(),
            focus_address: false,
            cookie_script_id: None,
            group: 0,
            recorded: String::new(),
            id,
            controller: None,
            webview: None,
            doc_url: url.clone(),
            url,
            title: String::new(),
            loading: false,
            blocked: 0,
            last_active: Instant::now(),
            suspended: false,
            creating: false,
            doc_script_id: None,
            pending_new_window: None,
        }
    }
}

pub struct Ext {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub handle: ICoreWebView2BrowserExtension,
}

pub struct State {
    pub hwnd: HWND,
    pub ui: Ui,
    pub env: Option<ICoreWebView2Environment>,
    pub browser_version: String,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub next_id: u32,
    /// Recently closed tabs: (url, position) for Ctrl+Shift+T.
    pub closed: Vec<(String, usize)>,
    /// A Rust rebuild (Update button) is running.
    pub building: bool,
    /// Tab groups and the next free group id.
    pub groups: Vec<Group>,
    pub next_group: u32,
    /// Extra window made by dragging a tab out: it doesn't restore or save the
    /// session or the window position (the main window does that).
    pub secondary: bool,
    pub settings: Settings,
    pub extensions: Vec<Ext>,
    pub extensions_loaded: bool,
    pub fullscreen: Option<(i32, WINDOWPLACEMENT)>,
    pub dpi: u32,
    pub minimized: bool,
}

impl State {
    pub fn tab_index(&self, id: u32) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }
    pub fn tab(&self, id: u32) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }
    pub fn tab_mut(&mut self, id: u32) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }
    pub fn is_active(&self, id: u32) -> bool {
        self.active_tab().map(|t| t.id == id).unwrap_or(false)
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Main window handle, kept outside STATE so it can be read even while the
/// state is borrowed (e.g. from hooks called inside `with`).
static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

pub fn init(state: State) {
    MAIN_HWND.store(state.hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
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

pub fn hwnd() -> HWND {
    HWND(MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed) as *mut _)
}

thread_local! {
    static LATER: std::cell::RefCell<Vec<Box<dyn FnOnce()>>> = std::cell::RefCell::new(Vec::new());
}

/// Runs `f` on the UI thread after the current event (never inside a WebView
/// event, so dialogs and file windows are safe there).
pub fn later(f: impl FnOnce() + 'static) {
    LATER.with(|l| l.borrow_mut().push(Box::new(f)));
    crate::util::post(hwnd(), WM_APP_LATER, 0, 0);
}

/// WM_APP_LATER: run what is queued.
pub fn run_later() {
    let list = LATER.with(|l| std::mem::take(&mut *l.borrow_mut()));
    for f in list {
        f();
    }
}