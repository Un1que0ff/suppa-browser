// settings.ini and session.txt in %LOCALAPPDATA%\SuppaBrowser.
use crate::util::data_dir;

pub const SEARCH_ENGINES: [(&str, &str); 4] = [
    ("Google", "https://www.google.com/search"),
    ("DuckDuckGo", "https://duckduckgo.com/"),
    ("Bing", "https://www.bing.com/search"),
    ("Brave Search", "https://search.brave.com/search"),
];

pub struct Settings {
    pub search: usize,
    pub adblock: bool,
    /// Background tabs are frozen (no CPU) after this many minutes.
    pub sleep_minutes: u64,
    /// Background tabs are fully unloaded (frees RAM) after this many minutes. 0 = never.
    pub discard_minutes: u64,
    pub restore_session: bool,
    pub dark: bool,
    /// Page for new tabs and the Home button.
    pub home: String,
    /// "cycle" = a different accent colour per tab, or "#rrggbb".
    pub accent: String,
    /// Click "Accept all" on cookie banners automatically (scripts\cookies.js).
    pub auto_cookies: bool,
    /// Offer to save passwords and fill them in.
    pub save_passwords: bool,
    /// Fill in addresses / names in forms.
    pub autofill: bool,
    /// Saved-tabs bar under the address bar.
    pub show_saved: bool,
    /// Ask on start to make Suppa the default browser.
    pub ask_default: bool,
    /// Heights (CSS px) of the tabs row, address-bar row and saved-tabs row.
    pub row_tabs: i32,
    pub row_nav: i32,
    pub row_saved: i32,
    /// Rows can't be resized by dragging.
    pub lock_sizes: bool,
    /// Corner roundness in px (0 = square). Tabs use it as is; buttons, menus
    /// etc. follow it (see --radius in ui.css / menu.html / settings.html).
    pub radius: i32,
    /// Interface scale in percent (tabs, address bar, menus; not web pages).
    pub ui_scale: i32,
    /// Text/icon colour on accent backgrounds: "auto", "black" or "white".
    pub on_accent: String,
    /// Layout: where the bar is: "top", "bottom", "left" or "right".
    /// (Older setting; each row now has its own place, see tabs_pos etc.)
    pub bar_pos: String,
    /// Layout: where each row is ("top", "bottom", "left" or "right").
    pub tabs_pos: String,
    pub nav_pos: String,
    pub saved_pos: String,
    /// Theme: "dark", "darker", "light" or "lighter" (dark = dark or darker).
    pub theme: String,
    /// Tint over the whole interface: "#rrggbb" and how strong in percent (0 = off).
    pub tint: String,
    pub tint_amount: i32,
    /// Which tab corners are rounded: "auto" (the ones away from the page) or
    /// four 0/1 flags in the order top-left, top-right, bottom-right, bottom-left.
    pub tab_corners: String,
    /// Corner shape of the whole interface: "round" or "squircle".
    pub corner_shape: String,
    /// Icon style: "line", "bold", "sharp", "filled" or "duotone".
    pub icon_pack: String,
    /// Layout: width of the bar when it is on the left/right (CSS px).
    pub side_width: i32,
    /// Layout: order of the bar rows ("tabs", "nav", "saved").
    pub row_order: Vec<String>,
    /// Pro settings -> Colors: custom colours per theme, (name, "#rrggbb").
    /// Names are the CSS colour variables, see COLOR_NAMES.
    pub colors_dark: Vec<(String, String)>,
    pub colors_light: Vec<(String, String)>,
    /// Window position/size when not maximized (left, top, right, bottom) + maximized.
    pub window: Option<(i32, i32, i32, i32)>,
    pub maximized: bool,
    /// Sites where the ad blocker is turned off.
    pub allow: Vec<String>,
    /// Extensions we unpacked ourselves: (extension id, folder).
    pub ext_folders: Vec<(String, String)>,
    /// Extensions not shown as buttons next to the address bar.
    pub ext_hidden: Vec<String>,
}

pub const DEFAULT_HOME: &str = "https://www.google.com/";
/// (min, default, max) heights in CSS px.
pub const ROW_TABS: (i32, i32, i32) = (28, 40, 72);
pub const ROW_NAV: (i32, i32, i32) = (34, 46, 80);
pub const ROW_SAVED: (i32, i32, i32) = (22, 30, 56);
/// Corner roundness (min, default, max) in px.
pub const RADIUS: (i32, i32, i32) = (0, 10, 20);
/// Interface scale (min, default, max) in percent.
pub const UI_SCALE: (i32, i32, i32) = (75, 100, 150);

/// Colours that can be changed in Pro settings (CSS variable names).
pub const COLOR_NAMES: [&str; 11] =
    ["bg", "panel", "field", "btn", "btn-hover", "tab", "tab-fg", "fg", "muted", "border", "incognito"];

/// Width of the side bar (min, default, max) in CSS px.
pub const SIDE_W: (i32, i32, i32) = (180, 260, 480);

pub fn valid_bar_pos(v: &str) -> String {
    match v.trim() {
        "bottom" => "bottom",
        "left" => "left",
        "right" => "right",
        _ => "top",
    }
    .to_string()
}

/// Tint strength (min, default, max) in percent.
pub const TINT: (i32, i32, i32) = (0, 0, 60);

pub fn valid_theme(v: &str) -> String {
    match v.trim() {
        "darker" => "darker",
        "light" => "light",
        "lighter" => "lighter",
        _ => "dark",
    }
    .to_string()
}

pub fn valid_tint(v: &str) -> String {
    let v = v.trim();
    if valid_hex(v) { v.to_ascii_lowercase() } else { "#ff0000".to_string() }
}

/// "auto" or four 0/1 flags (top-left, top-right, bottom-right, bottom-left).
pub fn valid_tab_corners(v: &str) -> String {
    let v = v.trim();
    if v.len() == 4 && v.chars().all(|c| c == '0' || c == '1') { v.to_string() } else { "auto".to_string() }
}

pub fn valid_icon_pack(v: &str) -> String {
    match v.trim() {
        p @ ("bold" | "sharp" | "filled" | "duotone") => p,
        _ => "line",
    }
    .to_string()
}

pub fn valid_corner_shape(v: &str) -> String {
    if v.trim() == "squircle" { "squircle" } else { "round" }.to_string()
}

pub fn default_order() -> Vec<String> {
    vec!["tabs".into(), "nav".into(), "saved".into()]
}

/// "nav,tabs,saved" -> list; anything that isn't the three rows once each -> default.
pub fn valid_order(v: &str) -> Vec<String> {
    let list: Vec<String> = v.split(',').map(|x| x.trim().to_string()).collect();
    let ok = list.len() == 3 && ["tabs", "nav", "saved"].iter().all(|k| list.iter().filter(|x| x == k).count() == 1);
    if ok { list } else { default_order() }
}

pub fn valid_on_accent(v: &str) -> String {
    match v.trim() {
        "black" => "black",
        "white" => "white",
        _ => "auto",
    }
    .to_string()
}

fn valid_hex(v: &str) -> bool {
    v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl Settings {
    /// Theme name that matches `dark` (keeps "darker"/"lighter" when they fit).
    pub fn theme_name(&self) -> &str {
        let t = self.theme.as_str();
        match (self.dark, t) {
            (true, "darker") | (false, "light") | (false, "lighter") => t,
            (true, _) => "dark",
            (false, _) => "light",
        }
    }
    /// Switches between light and dark, keeping the strength ("darker" <-> "lighter").
    pub fn set_dark(&mut self, dark: bool) {
        let extra = matches!(self.theme_name(), "darker" | "lighter");
        self.theme = match (dark, extra) {
            (true, true) => "darker",
            (true, false) => "dark",
            (false, true) => "lighter",
            (false, false) => "light",
        }
        .to_string();
        self.dark = dark;
    }
    pub fn set_theme(&mut self, theme: &str) {
        self.theme = valid_theme(theme);
        self.dark = matches!(self.theme.as_str(), "dark" | "darker");
    }
    /// Puts every row at one place (the older "bar position").
    pub fn set_bar_pos(&mut self, v: &str) {
        self.bar_pos = valid_bar_pos(v);
        self.tabs_pos = self.bar_pos.clone();
        self.nav_pos = self.bar_pos.clone();
        self.saved_pos = self.bar_pos.clone();
    }
    /// Sets the place of one row; bar_pos follows the address bar (for Linux).
    pub fn set_row_pos(&mut self, row: &str, v: &str) {
        let v = valid_bar_pos(v);
        match row {
            "tabs" => self.tabs_pos = v,
            "nav" => self.nav_pos = v,
            "saved" => self.saved_pos = v,
            _ => return,
        }
        self.bar_pos = self.nav_pos.clone();
    }
    /// Place of a row: "tabs", "nav" or "saved".
    pub fn row_pos(&self, row: &str) -> &str {
        match row {
            "tabs" => &self.tabs_pos,
            "nav" => &self.nav_pos,
            _ => &self.saved_pos,
        }
    }
    /// Custom colours of the current theme.
    pub fn colors(&self) -> &Vec<(String, String)> {
        if self.dark { &self.colors_dark } else { &self.colors_light }
    }
    /// Sets (or with None removes) a custom colour of the current theme.
    pub fn set_color(&mut self, name: &str, hex: Option<&str>) {
        if !COLOR_NAMES.contains(&name) {
            return;
        }
        let list = if self.dark { &mut self.colors_dark } else { &mut self.colors_light };
        list.retain(|(n, _)| n != name);
        if let Some(h) = hex.filter(|h| valid_hex(h)) {
            list.push((name.to_string(), h.to_ascii_lowercase()));
        }
    }
    pub fn reset_colors(&mut self) {
        if self.dark { self.colors_dark.clear() } else { self.colors_light.clear() }
    }
}

pub fn clamp_row(v: i32, r: (i32, i32, i32)) -> i32 {
    v.clamp(r.0, r.2)
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            home: DEFAULT_HOME.to_string(),
            accent: "cycle".to_string(),
            auto_cookies: true,
            save_passwords: true,
            autofill: true,
            show_saved: true,
            ask_default: true,
            row_tabs: ROW_TABS.1,
            row_nav: ROW_NAV.1,
            row_saved: ROW_SAVED.1,
            lock_sizes: false,
            radius: RADIUS.1,
            ui_scale: UI_SCALE.1,
            on_accent: "auto".to_string(),
            bar_pos: "top".to_string(),
            tabs_pos: "top".to_string(),
            nav_pos: "top".to_string(),
            saved_pos: "top".to_string(),
            theme: "dark".to_string(),
            tint: "#ff0000".to_string(),
            tint_amount: TINT.1,
            tab_corners: "auto".to_string(),
            corner_shape: "round".to_string(),
            icon_pack: "line".to_string(),
            side_width: SIDE_W.1,
            row_order: default_order(),
            colors_dark: Vec::new(),
            colors_light: Vec::new(),
            window: None,
            maximized: false,
            search: 0,
            adblock: true,
            sleep_minutes: 5,
            discard_minutes: 30,
            restore_session: true,
            dark: true,
            allow: Vec::new(),
            ext_folders: Vec::new(),
            ext_hidden: Vec::new(),
        }
    }
}

impl Settings {
    pub fn search_base(&self) -> &'static str {
        SEARCH_ENGINES[self.search.min(SEARCH_ENGINES.len() - 1)].1
    }
    pub fn is_allowed(&self, host: &str) -> bool {
        let host = host.strip_prefix("www.").unwrap_or(host);
        self.allow.iter().any(|a| host == a || host.ends_with(&format!(".{a}")))
    }
    pub fn toggle_allow(&mut self, host: &str) {
        let host = host.strip_prefix("www.").unwrap_or(host).to_string();
        if host.is_empty() {
            return;
        }
        if let Some(i) = self.allow.iter().position(|a| *a == host) {
            self.allow.remove(i);
        } else {
            self.allow.push(host);
        }
    }
    pub fn ext_folder(&self, id: &str) -> Option<String> {
        self.ext_folders.iter().find(|(i, _)| i == id).map(|(_, f)| f.clone())
    }
}

/// "cycle", "windows" (Windows accent colour) or a "#rrggbb" colour; anything
/// else becomes "cycle".
pub fn valid_accent(v: &str) -> String {
    let v = v.trim();
    if v == "windows" {
        return v.to_string();
    }
    let ok = v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok { v.to_ascii_lowercase() } else { "cycle".to_string() }
}

pub fn load() -> Settings {
    let mut s = Settings::default();
    let Ok(text) = std::fs::read_to_string(data_dir().join("settings.ini")) else {
        return s;
    };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        match k.trim() {
            "search" => s.search = v.parse().unwrap_or(0).min(SEARCH_ENGINES.len() - 1),
            "adblock" => s.adblock = v != "0",
            "sleep_minutes" => s.sleep_minutes = v.parse().unwrap_or(5).max(1),
            "discard_minutes" => s.discard_minutes = v.parse().unwrap_or(30),
            "restore_session" => s.restore_session = v != "0",
            "theme" => s.set_theme(v),
            "tint" => s.tint = valid_tint(v),
            "tint_amount" => s.tint_amount = clamp_row(v.parse().unwrap_or(TINT.1), TINT),
            "tab_corners" => s.tab_corners = valid_tab_corners(v),
            "corner_shape" => s.corner_shape = valid_corner_shape(v),
            "icon_pack" => s.icon_pack = valid_icon_pack(v),
            "home" => s.home = if v.is_empty() { DEFAULT_HOME.to_string() } else { v.to_string() },
            "accent" => s.accent = valid_accent(v),
            "cookies" => s.auto_cookies = v != "0",
            "passwords" => s.save_passwords = v != "0",
            "autofill" => s.autofill = v != "0",
            "saved_bar" => s.show_saved = v != "0",
            "ask_default" => s.ask_default = v != "0",
            "row_tabs" => s.row_tabs = clamp_row(v.parse().unwrap_or(ROW_TABS.1), ROW_TABS),
            "row_nav" => s.row_nav = clamp_row(v.parse().unwrap_or(ROW_NAV.1), ROW_NAV),
            "row_saved" => s.row_saved = clamp_row(v.parse().unwrap_or(ROW_SAVED.1), ROW_SAVED),
            "lock_sizes" => s.lock_sizes = v != "0",
            "radius" => s.radius = clamp_row(v.parse().unwrap_or(RADIUS.1), RADIUS),
            "ui_scale" => s.ui_scale = clamp_row(v.parse().unwrap_or(UI_SCALE.1), UI_SCALE),
            "on_accent" => s.on_accent = valid_on_accent(v),
            "bar_bottom" => {
                if v == "1" {
                    s.set_bar_pos("bottom"); // older settings files
                }
            }
            // Older files have only bar_pos: every row goes there.
            "bar_pos" => s.set_bar_pos(v),
            "tabs_pos" => s.tabs_pos = valid_bar_pos(v),
            "nav_pos" => s.nav_pos = valid_bar_pos(v),
            "saved_pos" => s.saved_pos = valid_bar_pos(v),
            "side_width" => s.side_width = clamp_row(v.parse().unwrap_or(SIDE_W.1), SIDE_W),
            "row_order" => s.row_order = valid_order(v),
            // color_dark=bg:#101010
            k @ ("color_dark" | "color_light") => {
                if let Some((name, hex)) = v.split_once(':') {
                    if COLOR_NAMES.contains(&name) && valid_hex(hex) {
                        let list = if k == "color_dark" { &mut s.colors_dark } else { &mut s.colors_light };
                        list.retain(|(n, _)| n != name);
                        list.push((name.to_string(), hex.to_ascii_lowercase()));
                    }
                }
            }
            "maximized" => s.maximized = v != "0",
            "window" => {
                let n: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if n.len() == 4 && n[2] - n[0] >= 200 && n[3] - n[1] >= 150 {
                    s.window = Some((n[0], n[1], n[2], n[3]));
                }
            }
            "allow" => {
                s.allow = v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
            }
            "ext_hidden" => {
                if !v.is_empty() {
                    s.ext_hidden.push(v.to_string());
                }
            }
            "ext" => {
                if let Some((id, folder)) = v.split_once('|') {
                    s.ext_folders.push((id.to_string(), folder.to_string()));
                }
            }
            _ => {}
        }
    }
    s
}

pub fn save(s: &Settings) {
    let mut out = String::new();
    out.push_str("# Suppa Browser settings\n");
    out.push_str(&format!("search={}\n", s.search));
    out.push_str(&format!("adblock={}\n", s.adblock as u8));
    out.push_str(&format!("sleep_minutes={}\n", s.sleep_minutes));
    out.push_str(&format!("discard_minutes={}\n", s.discard_minutes));
    out.push_str(&format!("restore_session={}\n", s.restore_session as u8));
    out.push_str(&format!("theme={}\n", s.theme_name()));
    out.push_str(&format!("tint={}\ntint_amount={}\n", s.tint, s.tint_amount));
    out.push_str(&format!("tab_corners={}\ncorner_shape={}\nicon_pack={}\n", s.tab_corners, s.corner_shape, s.icon_pack));
    out.push_str(&format!("home={}\n", s.home));
    out.push_str(&format!("accent={}\n", s.accent));
    out.push_str(&format!("cookies={}\n", s.auto_cookies as u8));
    out.push_str(&format!("passwords={}\n", s.save_passwords as u8));
    out.push_str(&format!("autofill={}\n", s.autofill as u8));
    out.push_str(&format!("saved_bar={}\n", s.show_saved as u8));
    out.push_str(&format!("ask_default={}\n", s.ask_default as u8));
    out.push_str(&format!("row_tabs={}\nrow_nav={}\nrow_saved={}\n", s.row_tabs, s.row_nav, s.row_saved));
    out.push_str(&format!("lock_sizes={}\n", s.lock_sizes as u8));
    out.push_str(&format!("radius={}\n", s.radius));
    out.push_str(&format!("ui_scale={}\n", s.ui_scale));
    out.push_str(&format!("on_accent={}\n", s.on_accent));
    out.push_str(&format!("bar_pos={}\nside_width={}\nrow_order={}\n", s.bar_pos, s.side_width, s.row_order.join(",")));
    out.push_str(&format!("tabs_pos={}\nnav_pos={}\nsaved_pos={}\n", s.tabs_pos, s.nav_pos, s.saved_pos));
    for (n, h) in &s.colors_dark {
        out.push_str(&format!("color_dark={n}:{h}\n"));
    }
    for (n, h) in &s.colors_light {
        out.push_str(&format!("color_light={n}:{h}\n"));
    }
    if let Some((l, t, r, b)) = s.window {
        out.push_str(&format!("window={l},{t},{r},{b}\n"));
    }
    out.push_str(&format!("maximized={}\n", s.maximized as u8));
    out.push_str(&format!("allow={}\n", s.allow.join(",")));
    for (id, folder) in &s.ext_folders {
        out.push_str(&format!("ext={id}|{folder}\n"));
    }
    for id in &s.ext_hidden {
        out.push_str(&format!("ext_hidden={id}\n"));
    }
    let _ = std::fs::write(data_dir().join("settings.ini"), out);
}

/// Returns (urls, active index).
pub fn load_session() -> (Vec<String>, usize) {
    let Ok(text) = std::fs::read_to_string(data_dir().join("session.txt")) else {
        return (Vec::new(), 0);
    };
    let mut lines = text.lines();
    let active = lines.next().and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    // Lines starting with '#' carry extra info (tab groups), see load_session_meta.
    let urls: Vec<String> = lines.map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let active = active.min(urls.len().saturating_sub(1));
    (urls, active)
}

/// Extra '#' lines of session.txt (tab groups).
pub fn load_session_meta() -> Vec<String> {
    std::fs::read_to_string(data_dir().join("session.txt"))
        .map(|t| t.lines().skip(1).filter(|l| l.starts_with('#')).map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn save_session(urls: &[String], active: usize, meta: &[String]) {
    let mut out = format!("{active}\n");
    for u in urls {
        out.push_str(u);
        out.push('\n');
    }
    for m in meta {
        out.push_str(m);
        out.push('\n');
    }
    let _ = std::fs::write(data_dir().join("session.txt"), out);
}
