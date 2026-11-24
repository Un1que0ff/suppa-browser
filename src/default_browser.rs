// "Make Suppa the default browser".
// Windows does not let programs make themselves the default browser. So Suppa
// registers itself as a browser for the current user (HKCU, no admin rights),
// then opens Windows Settings -> Default apps, where you pick Suppa.
// On start, a themed popup asks once (until "Don't ask again" or it is default).
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
use windows::core::{HSTRING, PCWSTR};

use crate::popup::{self, Item};
use crate::{app, scripts, settings, util};

const PROG_ID: &str = "SuppaHTML";
const APP_KEY: &str = r"Software\Clients\StartMenuInternet\Suppa";

fn set_str(path: &str, name: Option<&str>, value: &str) -> bool {
    unsafe {
        let mut key = HKEY::default();
        let r = RegCreateKeyW(HKEY_CURRENT_USER, &HSTRING::from(path), &mut key);
        if r.is_err() {
            return false;
        }
        let data: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
        let name_h = name.map(HSTRING::from);
        let name_p = name_h.as_ref().map(|h| PCWSTR(h.as_ptr())).unwrap_or(PCWSTR::null());
        let ok = RegSetValueExW(key, name_p, None, REG_SZ, Some(bytes)).is_ok();
        let _ = RegCloseKey(key);
        ok
    }
}

fn get_str(path: &str, name: &str) -> Option<String> {
    unsafe {
        let mut buf = vec![0u16; 260];
        let mut size = (buf.len() * 2) as u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(path),
            &HSTRING::from(name),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        if r.is_err() {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// True when Windows opens http/https links with Suppa.
pub fn is_default() -> bool {
    let p = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice";
    get_str(p, "ProgId").map(|id| id == PROG_ID).unwrap_or(false)
}

/// Registers Suppa as a browser for this user (links, .htm/.html files).
fn register() -> bool {
    let exe = scripts::exe_path().display().to_string();
    let open = format!("\"{exe}\" \"%1\"");
    let icon = format!("{exe},0");
    let caps = format!(r"{APP_KEY}\Capabilities");
    let mut ok = true;
    ok &= set_str(&format!(r"Software\Classes\{PROG_ID}"), None, "Suppa HTML Document");
    ok &= set_str(&format!(r"Software\Classes\{PROG_ID}"), Some("URL Protocol"), "");
    ok &= set_str(&format!(r"Software\Classes\{PROG_ID}\DefaultIcon"), None, &icon);
    ok &= set_str(&format!(r"Software\Classes\{PROG_ID}\shell\open\command"), None, &open);
    ok &= set_str(APP_KEY, None, "Suppa");
    ok &= set_str(&format!(r"{APP_KEY}\DefaultIcon"), None, &icon);
    ok &= set_str(&format!(r"{APP_KEY}\shell\open\command"), None, &format!("\"{exe}\""));
    ok &= set_str(&caps, Some("ApplicationName"), "Suppa");
    ok &= set_str(&caps, Some("ApplicationDescription"), "Suppa Browser - small, fast, with a built-in ad blocker");
    ok &= set_str(&caps, Some("ApplicationIcon"), &icon);
    ok &= set_str(&format!(r"{caps}\StartMenu"), Some("StartMenuInternet"), "Suppa");
    for proto in ["http", "https"] {
        ok &= set_str(&format!(r"{caps}\URLAssociations"), Some(proto), PROG_ID);
    }
    for ext in [".htm", ".html", ".xhtml", ".svg"] {
        ok &= set_str(&format!(r"{caps}\FileAssociations"), Some(ext), PROG_ID);
    }
    ok &= set_str(r"Software\RegisteredApplications", Some("Suppa"), &caps);
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    ok
}

/// Registers Suppa and opens Windows' Default apps page.
pub fn make_default() {
    if !register() {
        util::info(app::hwnd(), "Could not register Suppa as a browser");
        return;
    }
    util::log("registered as a browser; opening Default apps");
    // Windows 11 opens Suppa's own page; Windows 10 opens Default apps.
    util::open_in_explorer(std::path::Path::new("ms-settings:defaultapps?registeredAppUser=Suppa"));
    util::info(
        app::hwnd(),
        "Windows Settings is open.\n\nUnder \"Web browser\", click the current browser and choose Suppa.",
    );
}

/// On start: ask once with a themed popup, unless Suppa already is the default.
pub fn maybe_ask(anchor: windows::Win32::Foundation::POINT) {
    let ask = app::with(|s| s.settings.ask_default).unwrap_or(false);
    if !ask || is_default() {
        return;
    }
    let items = vec![
        Item::new(0, "Suppa is not your default browser").enabled(false),
        Item::sep(),
        Item::new(1, "Make Suppa the default browser").hot(true),
        Item::new(2, "Not now"),
        Item::new(3, "Don't ask again"),
    ];
    popup::show("default", items, anchor, true, |pick| match pick {
        Some(1) => make_default(),
        Some(3) => {
            app::with(|s| {
                s.settings.ask_default = false;
                settings::save(&s.settings);
            });
        }
        _ => {}
    });
}
