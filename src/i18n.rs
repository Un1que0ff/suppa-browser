// Interface language (Settings -> Appearance -> Language): English, Russian,
// German, Spanish or Chinese. "auto" (the default) = the Windows display
// language when Suppa has it, else English.
// Menus and dialogs built here use `tr("English text")`; Suppa's own pages
// (bar, menus, Settings) translate themselves with scripts\i18n.js. Both read
// scripts\lang.json.
use std::sync::OnceLock;

use crate::app;

pub const LANGS: [&str; 5] = ["en", "ru", "de", "es", "zh"];

fn dict() -> &'static serde_json::Value {
    static D: OnceLock<serde_json::Value> = OnceLock::new();
    D.get_or_init(|| serde_json::from_str(crate::scripts::read_text("lang.json").trim_start_matches('\u{feff}')).unwrap_or_default())
}

/// The Windows display language, if Suppa has it.
pub fn windows_lang() -> &'static str {
    let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    match id & 0x3ff {
        0x19 => "ru",
        0x07 => "de",
        0x0a => "es",
        0x04 => "zh",
        _ => "en",
    }
}

/// "auto" -> the Windows language; anything unknown -> English.
pub fn resolve(setting: &str) -> &'static str {
    match LANGS.iter().find(|l| **l == setting) {
        Some(l) => l,
        None => windows_lang(),
    }
}

/// The language in use now.
pub fn lang() -> &'static str {
    let setting = app::with(|s| s.settings.language.clone()).unwrap_or_default();
    resolve(&setting)
}

/// One text in the current language (unchanged when there is no translation).
pub fn tr(en: &str) -> String {
    let l = lang();
    if l == "en" {
        return en.to_string();
    }
    dict()[l][en].as_str().unwrap_or(en).to_string()
}

/// A text with "{}" places: translated, then the values put in, in order.
pub fn trf(en: &str, args: &[&str]) -> String {
    let mut s = tr(en);
    for a in args {
        if let Some(i) = s.find("{}") {
            s.replace_range(i..i + 2, a);
        }
    }
    s
}

/// For Settings: (code, name) of every language.
pub fn names() -> Vec<(String, String)> {
    LANGS
        .iter()
        .map(|l| (l.to_string(), dict()["languages"][*l].as_str().unwrap_or(l).to_string()))
        .collect()
}
