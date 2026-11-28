// Keyboard shortcuts -> command IDs. Used both when a web page has focus
// (WebView2 AcceleratorKeyPressed) and when the toolbar has focus.
// The same keys as in Chrome, Edge and Firefox. Pages handle Ctrl+F (find),
// Ctrl+P (print), F12 / Ctrl+Shift+I (developer tools) themselves.
// ≡ -> Help -> Keyboard shortcuts lists them all (SHORTCUTS below).
use crate::app::{self, *};

pub fn map(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Option<u16> {
    const VK_TAB: u32 = 0x09;
    const VK_ESC: u32 = 0x1B;
    const VK_PRIOR: u32 = 0x21;
    const VK_NEXT: u32 = 0x22;
    const VK_HOME: u32 = 0x24;
    const VK_LEFT: u32 = 0x25;
    const VK_RIGHT: u32 = 0x27;
    const VK_F4: u32 = 0x73;
    const VK_F10: u32 = 0x79;
    const VK_DELETE: u32 = 0x2E;
    const VK_F5: u32 = 0x74;
    const VK_F6: u32 = 0x75;
    const VK_F11: u32 = 0x7A;
    const VK_PLUS: u32 = 0xBB;
    const VK_MINUS: u32 = 0xBD;
    const VK_ADD: u32 = 0x6B;
    const VK_SUBTRACT: u32 = 0x6D;
    let key = |c: char| c as u32;

    let cmd = match (ctrl, shift, alt, vk) {
        (true, false, false, v) if v == key('T') || v == key('N') => CMD_NEW_TAB,
        (true, true, false, v) if v == key('T') => CMD_REOPEN_TAB,
        (true, true, false, v) if v == key('N') => CMD_NEW_INCOGNITO,
        (false, false, true, v) if v == key('S') => CMD_SETTINGS, // Alt+S
        (true, false, false, v) if v == key('W') || v == VK_F4 => CMD_CLOSE_TAB,
        (true, false, false, VK_TAB) | (true, false, false, VK_NEXT) => CMD_NEXT_TAB,
        (true, true, false, VK_TAB) | (true, false, false, VK_PRIOR) => CMD_PREV_TAB,
        (true, false, false, v) if (key('1')..=key('9')).contains(&v) => CMD_TAB_BASE + (v - key('1')) as u16,
        (true, false, false, v) if v == key('L') => CMD_FOCUS_ADDRESS,
        (false, false, true, v) if v == key('D') => CMD_FOCUS_ADDRESS,
        (false, false, false, VK_F6) => CMD_FOCUS_ADDRESS,
        (false, false, false, VK_F11) => CMD_FULLSCREEN,
        (true, false, false, v) if v == key('J') => CMD_DOWNLOADS,
        (true, false, false, v) if v == key('H') => CMD_HISTORY,
        (false, true, false, VK_ESC) => CMD_TASK_MANAGER,
        (false, false, false, VK_ESC) if app::with(|s| s.fullscreen.is_some()).unwrap_or(false) => CMD_ESCAPE,
        (false, false, true, VK_HOME) => CMD_HOME,
        (false, false, false, VK_F5) => CMD_RELOAD,
        (true, false, false, v) if v == key('R') => CMD_RELOAD,
        (true, true, false, v) if v == key('R') => CMD_HARD_RELOAD,
        (true, false, false, VK_F5) => CMD_HARD_RELOAD,
        (false, false, true, VK_LEFT) => CMD_BACK,
        (false, false, true, VK_RIGHT) => CMD_FORWARD,
        (true, false, false, VK_PLUS) | (true, false, false, VK_ADD) => CMD_ZOOM_IN,
        (true, false, false, VK_MINUS) | (true, false, false, VK_SUBTRACT) => CMD_ZOOM_OUT,
        (true, false, false, v) if v == key('0') => CMD_ZOOM_RESET,
        // Chrome / Edge / Firefox keys. Ctrl+D, Ctrl+E / K, Ctrl+M and Ctrl+U come
        // from the page instead (scripts\media.js), so a page that uses them
        // itself (an editor's Ctrl+K link, Ctrl+U underline) keeps them, as in Chrome.
        (true, true, false, v) if v == key('B') => CMD_TOGGLE_SAVED_BAR,
        (true, true, false, VK_DELETE) => CMD_CLEAR_DATA,
        (false, false, true, v) if v == key('F') || v == key('E') => CMD_OPEN_MENU_KEY,
        (false, false, false, VK_F10) => CMD_OPEN_MENU_KEY,
        (true, true, false, VK_PRIOR) => CMD_MOVE_TAB_LEFT,
        (true, true, false, VK_NEXT) => CMD_MOVE_TAB_RIGHT,
        (true, true, false, v) if v == key('W') => CMD_CLOSE_ALL,
        (true, false, false, v) if v == key('O') => CMD_OPEN_FILE,
        (true, true, false, v) if v == key('M') => CMD_PLAYER,
        (true, true, false, v) if v == key('G') => CMD_GEN_PASSWORD,
        (true, false, false, 0xBC) => CMD_SETTINGS, // Ctrl+,
        (false, false, false, 0x70) => CMD_SHORTCUTS, // F1
        _ => return None,
    };
    Some(cmd)
}

/// For ≡ -> Help -> Keyboard shortcuts: (keys, what they do).
pub const SHORTCUTS: [(&str, &str); 40] = [
    ("Ctrl+T / Ctrl+N", "New tab"),
    ("Ctrl+Shift+N", "New incognito tab"),
    ("Ctrl+W / Ctrl+F4", "Close tab"),
    ("Ctrl+Shift+W", "Close the window"),
    ("Ctrl+Shift+T", "Reopen closed tab"),
    ("Ctrl+Tab / Ctrl+PgDn", "Next tab"),
    ("Ctrl+Shift+Tab / Ctrl+PgUp", "Previous tab"),
    ("Ctrl+1 ... Ctrl+8", "Go to tab 1 ... 8"),
    ("Ctrl+9", "Go to the last tab"),
    ("Ctrl+Shift+PgUp / PgDn", "Move the tab left / right"),
    ("Ctrl+M", "Mute / unmute the tab"),
    ("Ctrl+L / Alt+D / F6", "Address bar"),
    ("Ctrl+E / Ctrl+K", "Search in the address bar"),
    ("Ctrl+Enter (address bar)", "Add www. and .com"),
    ("Alt+Left / Alt+Right", "Back / forward"),
    ("F5 / Ctrl+R", "Reload"),
    ("Ctrl+F5 / Ctrl+Shift+R", "Reload without the cache"),
    ("Alt+Home", "Home page"),
    ("Ctrl+D", "Save the page to the saved tabs bar"),
    ("Ctrl+Shift+B", "Show / hide the saved tabs bar"),
    ("Ctrl+H", "History"),
    ("Ctrl+J", "Downloads"),
    ("Ctrl+Shift+M", "Player (what plays in your tabs)"),
    ("Ctrl+Shift+G", "Copy a new strong password"),
    ("Ctrl+F", "Find on the page"),
    ("Ctrl+P", "Print"),
    ("Ctrl+O", "Open a file"),
    ("Ctrl+U", "Page source"),
    ("Ctrl+ + / Ctrl+ - / Ctrl+0", "Zoom in / out / reset"),
    ("F11", "Full screen"),
    ("F12 / Ctrl+Shift+I", "Developer tools"),
    ("Shift+Esc", "Task manager"),
    ("Ctrl+Shift+Delete", "Clear browsing data"),
    ("Alt+F / Alt+E / F10", "Open the ≡ menu"),
    ("Alt+S / Ctrl+,", "Settings"),
    ("F1", "This list"),
    ("Middle click on a tab", "Close it"),
    ("Middle / Ctrl+click a link", "Open in a background tab"),
    ("Double-click the tab row", "Maximize / restore the window"),
    ("Drag a tab out of the window", "Open it in a new window"),
];

/// Keys a page did not use itself (scripts\media.js sends "suppa-key:<name>").
pub fn page_key(name: &str) -> Option<u16> {
    Some(match name {
        "star" => CMD_STAR,
        "search" => CMD_FOCUS_ADDRESS,
        "mute" => CMD_MUTE_TAB,
        "source" => CMD_VIEW_SOURCE,
        _ => return None,
    })
}
