// Keyboard shortcuts -> command IDs. Used both when a web page has focus
// (WebView2 AcceleratorKeyPressed) and when the toolbar has focus.
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
        (true, false, false, 0xBC) => CMD_SETTINGS, // Ctrl+,
        (true, false, false, v) if v == key('W') || v == VK_F4 => CMD_CLOSE_TAB,
        (true, false, false, VK_TAB) | (true, false, false, VK_NEXT) => CMD_NEXT_TAB,
        (true, true, false, VK_TAB) | (true, false, false, VK_PRIOR) => CMD_PREV_TAB,
        (true, false, false, v) if (key('1')..=key('9')).contains(&v) => CMD_TAB_BASE + (v - key('1')) as u16,
        (true, false, false, v) if v == key('L') => CMD_FOCUS_ADDRESS,
        (false, false, true, v) if v == key('D') => CMD_FOCUS_ADDRESS,
        (false, false, false, VK_F6) => CMD_FOCUS_ADDRESS,
        (false, false, false, VK_F11) => CMD_FULLSCREEN,
        (true, false, false, v) if v == key('J') => CMD_DOWNLOADS,
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
        _ => return None,
    };
    Some(cmd)
}
