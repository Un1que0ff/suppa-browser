// The main window: GTK window with the bar and the page area, commands,
// keyboard shortcuts, downloads, session, Update/restart.
use std::cell::Cell;
use std::path::PathBuf;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use crate::wk::*;
use webkit2gtk::{WebContext, WebsiteDataManager};

use crate::app::{self, *};
use crate::popup::{self, Item};
use crate::{adblock, scripts, settings, tabs, ui, util};

/// What this process was started with.
pub struct Launch {
    pub urls: Vec<String>,
    /// Extra window (a tab dragged out): no session restore / save.
    pub secondary: bool,
    pub incognito: bool,
}

// ---- Web contexts (profile) ---------------------------------------------------------------

/// Serves suppa://ui/..., suppa://settings/ and suppa://newtab/ from the scripts folder.
fn register_scheme(ctx: &WebContext) {
    ctx.register_uri_scheme("suppa", |req| {
        let uri = req.uri().map(|u| u.to_string()).unwrap_or_default();
        let rest = uri.strip_prefix("suppa://").unwrap_or("");
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let file = match host {
            "settings" => "settings.html".to_string(),
            "newtab" => "newtab.html".to_string(),
            "ui" => if path.is_empty() { "ui.html".to_string() } else { path.to_string() },
            _ => String::new(),
        };
        let served = scripts::serve(&file).map(|(mime, bytes)| {
            if file != "newtab.html" {
                return (mime, bytes);
            }
            // The start page's blanks: search engine and version (no shortcuts
            // or theme colours on Linux yet).
            let search = crate::app::with(|s| s.settings.search_base().to_string())
                .unwrap_or_else(|| crate::settings::SEARCH_ENGINES[0].1.to_string());
            let html = String::from_utf8_lossy(&bytes)
                .replace("{{SEARCH}}", &search)
                .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
                .replace("{{THEME}}", "")
                .replace("{{SHORTCUTS}}", "[]");
            (mime, html.into_bytes())
        });
        match served {
            Some((mime, bytes)) => {
                let len = bytes.len() as i64;
                let stream = gtk::gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                req.finish(&stream, len, Some(mime));
            }
            None => {
                let mut e = glib::Error::new(gtk::gio::IOErrorEnum::NotFound, "not found");
                req.finish_error(&mut e);
            }
        }
    });
    if let Some(sm) = ctx.security_manager() {
        // Web pages cannot load Suppa's own pages.
        sm.register_uri_scheme_as_local("suppa");
        sm.register_uri_scheme_as_secure("suppa");
    }
    ctx.connect_download_started(|_, dl| on_download(dl));
}

fn main_context() -> WebContext {
    let data = util::sub_dir("web");
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("SuppaBrowser");
    let dm = WebsiteDataManager::builder()
        .base_data_directory(data.to_string_lossy().as_ref())
        .base_cache_directory(cache.to_string_lossy().as_ref())
        .build();
    dm.set_itp_enabled(true); // Intelligent Tracking Prevention
    if let Some(cm) = dm.cookie_manager() {
        let file = data.join("cookies.sqlite");
        cm.set_persistent_storage(&file.to_string_lossy(), webkit2gtk::CookiePersistentStorage::Sqlite);
    }
    let ctx = WebContext::with_website_data_manager(&dm);
    ctx.set_cache_model(webkit2gtk::CacheModel::WebBrowser);
    register_scheme(&ctx);
    ctx
}

/// Context of the private (incognito) tabs: nothing is written to disk.
pub fn private_context() -> WebContext {
    if let Some(Some(c)) = app::with(|s| s.private_ctx.clone()) {
        return c;
    }
    let dm = WebsiteDataManager::new_ephemeral();
    let ctx = WebContext::with_website_data_manager(&dm);
    register_scheme(&ctx);
    app::with(|s| s.private_ctx = Some(ctx.clone()));
    ctx
}

pub fn engine_version() -> String {
    format!("WebKitGTK {}.{}.{}", unsafe { webkit2gtk::ffi::webkit_get_major_version() }, unsafe { webkit2gtk::ffi::webkit_get_minor_version() }, unsafe { webkit2gtk::ffi::webkit_get_micro_version() })
}

// ---- Window -----------------------------------------------------------------------------

fn lock_file() -> PathBuf {
    util::data_dir().join("running.lock")
}

/// Builds the window and opens the first tabs.
pub fn build(app_: &gtk::Application, launch: Launch) {
    let settings = settings::load();
    let crashed = !launch.secondary && lock_file().exists();
    if !launch.secondary {
        let _ = std::fs::write(lock_file(), std::process::id().to_string());
    }
    let ctx = main_context();

    let window = gtk::ApplicationWindow::new(app_);
    window.set_title("Suppa");
    window.set_icon_name(Some("suppa"));
    // No system title bar: the bar is the title bar. An empty, hidden title bar
    // widget keeps GTK's own resize borders and shadow around the window.
    let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    titlebar.set_no_show_all(true);
    window.set_titlebar(Some(&titlebar));
    match settings.window {
        Some((l, t, r, b)) if !launch.secondary => {
            window.set_default_size(r - l, b - t);
            window.move_(l, t);
        }
        _ => window.set_default_size(1280, 820),
    }
    if settings.maximized && !launch.secondary {
        window.maximize();
    }

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.style_context().add_class("suppa-root");
    let bar = ui::create(&ctx);
    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::None);
    pages.set_hexpand(true);
    pages.set_vexpand(true);
    root.pack_start(&bar, false, false, 0);
    root.pack_start(&pages, true, true, 0);
    window.add(&root);

    let dark = settings.dark;
    app::init(State {
        window: window.clone().upcast(),
        root,
        bar,
        bar_ready: false,
        pages,
        ctx,
        private_ctx: None,
        tabs: Vec::new(),
        active: 0,
        next_id: 1,
        closed: Vec::new(),
        building: false,
        update_available: false,
        groups: Vec::new(),
        next_group: 1,
        secondary: launch.secondary,
        settings,
        fullscreen: false,
        maximized: false,
        accent: "#b07cff".into(),
    });
    apply_theme(dark, "");
    connect_window(&window);

    adblock::start(false);

    // First tabs: links from the command line, else the last session.
    let restore = app::with(|s| s.settings.restore_session).unwrap_or(true) || crashed;
    if launch.incognito {
        for u in &launch.urls {
            tabs::new_incognito_url(u.clone());
        }
        if launch.urls.is_empty() {
            tabs::new_incognito();
        }
    } else {
        if !launch.secondary && restore {
            let (urls, active) = settings::load_session();
            if !urls.is_empty() {
                tabs::restore(urls, active);
                crate::groups::restore(&settings::load_session_meta());
                if crashed {
                    util::log("Suppa did not close normally last time - tabs restored");
                }
            }
        }
        for u in &launch.urls {
            tabs::new_tab(Some(u.clone()), true);
        }
    }
    if app::with(|s| s.tabs.is_empty()).unwrap_or(true) {
        tabs::new_tab(None, true);
    }
    window.show_all();
    ui::layout();
    popup::preload();
    start_timers();
}

fn connect_window(window: &gtk::ApplicationWindow) {
    window.connect_delete_event(|_, _| {
        close_window();
        glib::Propagation::Stop
    });
    window.connect_window_state_event(|_, ev| {
        let st = ev.new_window_state();
        app::with(|s| {
            s.maximized = st.contains(gdk::WindowState::MAXIMIZED);
        });
        ui::request_sync();
        glib::Propagation::Proceed
    });
    window.connect_key_press_event(|_, ev| {
        match shortcut(ev) {
            Some(cmd) => {
                command(cmd);
                glib::Propagation::Stop
            }
            None => glib::Propagation::Proceed,
        }
    });
    window.connect_focus_in_event(|_, _| {
        crate::saved::reload_if_changed();
        glib::Propagation::Proceed
    });
}

/// Dark/light: GTK widgets (scrollbars, form controls) and the window background.
pub fn apply_theme(dark: bool, bg: &str) {
    if let Some(st) = gtk::Settings::default() {
        st.set_gtk_application_prefer_dark_theme(dark);
    }
    let bg = if bg.starts_with('#') { bg.to_string() } else { if dark { "#161616" } else { "#f2f2f2" }.to_string() };
    thread_local! {
        static CSS: gtk::CssProvider = gtk::CssProvider::new();
    }
    CSS.with(|css| {
        let _ = css.load_from_data(format!(".suppa-root, window {{ background: {bg}; }}").as_bytes());
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(&screen, css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
    });
    if let Ok(c) = bg.parse::<gdk::RGBA>() {
        let views = app::with(|s| {
            let mut v: Vec<webkit2gtk::WebView> = s.tabs.iter().filter_map(|t| t.view.clone()).collect();
            v.push(s.bar.clone());
            v
        })
        .unwrap_or_default();
        for v in views {
            v.set_background_color(&c);
        }
    }
}

/// Moving / resizing the window from the bar (it has no system title bar).
pub fn begin_drag(edge: &str) {
    let Some(w) = app::with(|s| s.window.clone()) else { return };
    let p = popup::cursor_pos();
    let t = gtk::current_event_time();
    match edge {
        "top" => w.begin_resize_drag(gdk::WindowEdge::North, 1, p.x, p.y, t),
        "topleft" => w.begin_resize_drag(gdk::WindowEdge::NorthWest, 1, p.x, p.y, t),
        "topright" => w.begin_resize_drag(gdk::WindowEdge::NorthEast, 1, p.x, p.y, t),
        _ => w.begin_move_drag(1, p.x, p.y, t),
    }
}

pub fn set_fullscreen(on: bool) {
    let Some(w) = app::with(|s| {
        s.fullscreen = on;
        s.window.clone()
    }) else {
        return;
    };
    if on { w.fullscreen() } else { w.unfullscreen() }
    ui::layout();
}

/// Saves everything and closes this window (the program ends with it).
pub fn close_window() {
    popup::close();
    save_session_now();
    let r = app::with(|s| {
        if !s.secondary {
            let (x, y) = s.window.position();
            let (w, h) = s.window.size();
            s.settings.maximized = s.maximized;
            if !s.maximized && !s.fullscreen {
                s.settings.window = Some((x, y, x + w, y + h));
            }
            settings::save(&s.settings);
        }
        (s.window.clone(), s.secondary)
    });
    let Some((w, secondary)) = r else { return };
    if !secondary {
        let _ = std::fs::remove_file(lock_file());
    }
    // Tabs first, so their web processes end cleanly.
    app::with(|s| {
        for t in s.tabs.iter_mut() {
            t.view = None;
        }
    });
    unsafe { w.destroy() };
    if let Some(a) = gtk::gio::Application::default() {
        a.quit();
    }
}

// ---- Session ----------------------------------------------------------------------------

thread_local! {
    static SESSION_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Saves the open tabs shortly after they change (so a crash loses nothing).
pub fn save_session_soon() {
    if !SESSION_PENDING.with(|p| p.replace(true)) {
        glib::timeout_add_local_once(std::time::Duration::from_millis(800), || {
            SESSION_PENDING.with(|p| p.set(false));
            save_session_now();
        });
    }
}

fn save_session_now() {
    app::with(|s| {
        if s.secondary {
            return;
        }
        let mut urls = Vec::new();
        let mut meta = crate::groups::session_meta_groups(s);
        let mut active = 0;
        for (i, t) in s.tabs.iter().enumerate() {
            if t.incognito {
                continue;
            }
            if i == s.active {
                active = urls.len();
            }
            if t.group != 0 {
                meta.push(format!("#tabgroup\t{}\t{}", urls.len(), t.group));
            }
            urls.push(if t.url.is_empty() { s.settings.home.clone() } else { t.url.clone() });
        }
        settings::save_session(&urls, active, &meta);
    });
}

// ---- Timers -------------------------------------------------------------------------------

fn start_timers() {
    // The Update button lights up when the scripts folder or the Rust code changes.
    let sig = Cell::new(scripts::signature());
    glib::timeout_add_seconds_local(2, move || {
        let now = scripts::signature();
        if now != sig.get() {
            sig.set(now);
            ui::set_update_available(true);
        }
        glib::ControlFlow::Continue
    });
    // Unused background tabs are unloaded (Settings -> Unload after).
    glib::timeout_add_seconds_local(60, || {
        tabs::unload_idle();
        glib::ControlFlow::Continue
    });
}

// ---- Keyboard shortcuts ----------------------------------------------------------------

fn shortcut(ev: &gdk::EventKey) -> Option<u16> {
    use gdk::keys::constants as k;
    let m = ev.state();
    let ctrl = m.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = m.contains(gdk::ModifierType::SHIFT_MASK);
    let alt = m.contains(gdk::ModifierType::MOD1_MASK);
    let key = ev.keyval().to_lower();
    let fullscreen = app::with(|s| s.fullscreen).unwrap_or(false);
    let digit = |kv: &gdk::keys::Key| -> Option<u16> {
        let c = kv.to_unicode()?;
        c.to_digit(10).filter(|d| (1..=9).contains(d)).map(|d| d as u16)
    };
    let is = |a: &gdk::keys::Key| key == *a;
    Some(match (ctrl, shift, alt) {
        (true, false, false) if is(&k::t) || is(&k::n) => CMD_NEW_TAB,
        (true, true, false) if is(&k::t) || is(&k::T) => CMD_REOPEN_TAB,
        (true, true, false) if is(&k::n) || is(&k::N) => CMD_NEW_INCOGNITO,
        (false, false, true) if is(&k::s) => CMD_SETTINGS,
        (true, false, false) if is(&k::w) || is(&k::F4) => CMD_CLOSE_TAB,
        (true, false, false) if is(&k::Tab) || is(&k::Page_Down) => CMD_NEXT_TAB,
        (true, true, false) if is(&k::Tab) || is(&k::ISO_Left_Tab) => CMD_PREV_TAB,
        (true, false, false) if is(&k::Page_Up) => CMD_PREV_TAB,
        (true, false, false) if digit(&key).is_some() => CMD_TAB_BASE + digit(&key)? - 1,
        (true, false, false) if is(&k::l) => CMD_FOCUS_ADDRESS,
        (false, false, true) if is(&k::d) => CMD_FOCUS_ADDRESS,
        (false, false, false) if is(&k::F6) => CMD_FOCUS_ADDRESS,
        (false, false, false) if is(&k::F11) => CMD_FULLSCREEN,
        (false, false, false) if is(&k::F12) => CMD_DEVTOOLS,
        (true, true, false) if is(&k::i) || is(&k::I) => CMD_DEVTOOLS,
        (true, false, false) if is(&k::j) => CMD_DOWNLOADS,
        (true, false, false) if is(&k::p) => CMD_PRINT,
        (false, false, false) if is(&k::Escape) && fullscreen => CMD_ESCAPE,
        (false, false, true) if is(&k::Home) => CMD_HOME,
        (false, false, false) if is(&k::F5) => CMD_RELOAD,
        (true, false, false) if is(&k::r) => CMD_RELOAD,
        (true, true, false) if is(&k::r) || is(&k::R) => CMD_HARD_RELOAD,
        (true, false, false) if is(&k::F5) => CMD_HARD_RELOAD,
        (false, false, true) if is(&k::Left) => CMD_BACK,
        (false, false, true) if is(&k::Right) => CMD_FORWARD,
        (true, _, false) if is(&k::plus) || is(&k::equal) || is(&k::KP_Add) => CMD_ZOOM_IN,
        (true, false, false) if is(&k::minus) || is(&k::KP_Subtract) => CMD_ZOOM_OUT,
        (true, false, false) if is(&k::_0) || is(&k::KP_0) => CMD_ZOOM_RESET,
        _ => return None,
    })
}

// ---- Commands ----------------------------------------------------------------------------

pub fn command(cmd: u16) {
    let view = tabs::active_view();
    match cmd {
        CMD_NEW_TAB => {
            tabs::new_tab(None, true);
        }
        CMD_NEW_INCOGNITO => tabs::new_incognito(),
        CMD_CLOSE_TAB => tabs::close_active(),
        CMD_REOPEN_TAB => tabs::reopen_closed(),
        CMD_NEXT_TAB => tabs::select_relative(1),
        CMD_PREV_TAB => tabs::select_relative(-1),
        c if (CMD_TAB_BASE..CMD_TAB_BASE + 9).contains(&c) => {
            let n = app::with(|s| s.tabs.len()).unwrap_or(0);
            // Ctrl+9 = last tab, like other browsers.
            let i = if c == CMD_TAB_BASE + 8 { n.saturating_sub(1) } else { (c - CMD_TAB_BASE) as usize };
            if i < n {
                tabs::activate(i);
            }
        }
        CMD_BACK => {
            if let Some(v) = view {
                v.go_back();
            }
        }
        CMD_FORWARD => {
            if let Some(v) = view {
                v.go_forward();
            }
        }
        CMD_RELOAD => {
            if let Some(v) = view {
                v.reload();
            }
        }
        CMD_HARD_RELOAD => {
            if let Some(v) = view {
                v.reload_bypass_cache();
            }
        }
        CMD_HOME => {
            let home = app::with(|s| s.settings.home.clone()).unwrap_or_default();
            tabs::navigate_active(&home);
        }
        CMD_FOCUS_ADDRESS => ui::focus_address(),
        CMD_FULLSCREEN => {
            let on = !app::with(|s| s.fullscreen).unwrap_or(false);
            set_fullscreen(on);
        }
        CMD_ESCAPE => set_fullscreen(false),
        CMD_MINIMIZE => {
            if let Some(w) = app::with(|s| s.window.clone()) {
                w.iconify();
            }
        }
        CMD_MAXIMIZE => {
            if let Some((w, max)) = app::with(|s| (s.window.clone(), s.maximized)) {
                if max { w.unmaximize() } else { w.maximize() }
            }
        }
        CMD_CLOSE_WINDOW => close_window(),
        CMD_MENU => crate::menu::show_main(),
        CMD_ADBLOCK_MENU => crate::menu::show_adblock(),
        CMD_SETTINGS => {
            // An open Settings tab is reused.
            let open = app::with(|s| s.tabs.iter().position(|t| t.url.starts_with(tabs::SETTINGS_URL))).flatten();
            match open {
                Some(i) => tabs::activate(i),
                None => {
                    tabs::new_tab(Some(tabs::SETTINGS_URL.to_string()), true);
                }
            }
        }
        CMD_TOGGLE_THEME => {
            app::with(|s| {
                s.settings.dark = !s.settings.dark;
                settings::save(&s.settings);
            });
            ui::push_theme();
            crate::settings_page::broadcast();
        }
        CMD_LOCK_SIZES => {
            app::with(|s| {
                s.settings.lock_sizes = !s.settings.lock_sizes;
                settings::save(&s.settings);
            });
            ui::push_theme();
        }
        CMD_TOGGLE_ADBLOCK => {
            app::with(|s| {
                s.settings.adblock = !s.settings.adblock;
                settings::save(&s.settings);
            });
            adblock::reattach_all();
            tabs::apply_scripts_setting();
            if let Some(v) = view {
                v.reload();
            }
            ui::request_sync();
        }
        CMD_TOGGLE_SITE => {
            app::with(|s| {
                if let Some(host) = s.active_tab().map(|t| util::host_of(&t.url)) {
                    s.settings.toggle_allow(&host);
                    settings::save(&s.settings);
                }
            });
            adblock::reattach_all();
            if let Some(v) = view {
                v.reload();
            }
            ui::request_sync();
        }
        CMD_UPDATE_LISTS => {
            adblock::start(true);
            popup::message("Updating filter lists\n\nThe newest lists are downloaded in the background and used as soon as they are ready.");
        }
        CMD_DEVTOOLS => {
            if let Some(i) = view.and_then(|v| v.inspector()) {
                i.show();
            }
        }
        CMD_ZOOM_IN | CMD_ZOOM_OUT | CMD_ZOOM_RESET => {
            if let Some(v) = view {
                let z = match cmd {
                    CMD_ZOOM_IN => (v.zoom_level() + 0.1).min(5.0),
                    CMD_ZOOM_OUT => (v.zoom_level() - 0.1).max(0.3),
                    _ => 1.0,
                };
                v.set_zoom_level(z);
            }
        }
        CMD_PRINT => {
            if let Some(v) = view {
                let op = webkit2gtk::PrintOperation::new(&v);
                let w = app::with(|s| s.window.clone());
                op.run_dialog(w.as_ref());
            }
        }
        CMD_DOWNLOADS => util::open_path(&util::downloads_dir()),
        CMD_SLEEP_NOW => tabs::unload_all_background(),
        CMD_CLEAR_DATA => clear_data(),
        CMD_OPEN_LOG => util::open_path(&util::data_dir().join("suppa.log")),
        CMD_OPEN_SCRIPTS => util::open_path(scripts::dir()),
        CMD_OPEN_DATA_DIR => util::open_path(&util::data_dir()),
        CMD_ABOUT => about(),
        CMD_UPDATE => do_update(),
        CMD_MAKE_DEFAULT => make_default(),
        CMD_SAVED_RENAME => crate::saved::run_pending_rename(),
        CMD_DUPLICATE_TAB => {
            if let Some(i) = app::with(|s| s.active) {
                tabs::duplicate_index(i);
            }
        }
        CMD_CLOSE_OTHER_TABS => {
            if let Some(i) = app::with(|s| s.active) {
                tabs::close_others_except(i);
            }
        }
        _ => {}
    }
}

fn about() {
    let n = app::with(|s| s.tabs.len()).unwrap_or(0);
    popup::message(&format!(
        "About Suppa Browser\n\nVersion: {}\nPlatform: Linux ({})\nEngine: {}\nAd blocker: {}\nOpen tabs: {}\nData folder: {}\n\nhttps://github.com/Un1que0ff/suppa-browser",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        engine_version(),
        if adblock::is_ready() { "active" } else { "loading" },
        n,
        util::data_dir().display()
    ));
}

fn clear_data() {
    popup::confirm(
        "Clear browsing data?",
        "Cookies, cache, site storage and logins of all websites are deleted. Your tabs, saved tabs and settings stay.",
        "Clear",
        "Cancel",
        |yes| {
            if !yes {
                return;
            }
            if let Some(Some(dm)) = app::with(|s| s.ctx.website_data_manager()) {
                dm.clear(webkit2gtk::WebsiteDataTypes::ALL, glib::TimeSpan(0), None::<&gtk::gio::Cancellable>, |_| {
                    util::log("browsing data cleared");
                });
            }
        },
    );
}

// ---- Update / restart -----------------------------------------------------------------------

/// Update button: save tabs, close, and start again so every file in the
/// scripts folder (and a rebuilt binary) is loaded fresh. If the Rust code
/// changed, it is rebuilt first (in the background; Suppa keeps working).
fn do_update() {
    if app::with(|s| s.building).unwrap_or(false) {
        return;
    }
    if scripts::rust_changed() {
        app::with(|s| s.building = true);
        ui::post_update();
        std::thread::spawn(|| {
            let r = scripts::build_and_swap();
            glib::MainContext::default().invoke(move || {
                app::with(|s| s.building = false);
                ui::post_update();
                match r {
                    Ok(()) => restart(),
                    Err(e) => {
                        util::log(&format!("rebuild failed: {e}"));
                        popup::message(&format!(
                            "The new version could not be built\n\n{e}\n\nDetails: {}",
                            scripts::build_log().display()
                        ));
                    }
                }
            });
        });
    } else {
        restart();
    }
}

fn restart() {
    save_session_now();
    let exe = scripts::exe_path();
    let pid = std::process::id().to_string();
    if std::process::Command::new(exe).args(["--restart", &pid]).spawn().is_ok() {
        // The session is restored by the new process.
        app::with(|s| s.settings.restore_session = true);
        close_window();
    }
}

/// `--restart <pid>`: wait until the previous Suppa has ended.
pub fn wait_for_previous(pid: &str) {
    let p = PathBuf::from("/proc").join(pid);
    for _ in 0..100 {
        if !p.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

// ---- Downloads ------------------------------------------------------------------------------

fn on_download(dl: &webkit2gtk::Download) {
    dl.connect_decide_destination(|dl, name| {
        let dir = util::downloads_dir();
        let name = if name.trim().is_empty() { "download" } else { name };
        let name: String = name.chars().map(|c| if c == '/' { '_' } else { c }).collect();
        let mut path = dir.join(&name);
        let (stem, ext) = match name.rsplit_once('.') {
            Some((a, b)) if !a.is_empty() => (a.to_string(), format!(".{b}")),
            _ => (name.clone(), String::new()),
        };
        let mut n = 1;
        while path.exists() {
            path = dir.join(format!("{stem} ({n}){ext}"));
            n += 1;
        }
        if let Ok(uri) = glib::filename_to_uri(&path, None) {
            dl.set_destination(&uri);
        }
        util::log(&format!("download started: {}", path.display()));
        true
    });
    dl.connect_finished(|dl| {
        let Some(dest) = dl.destination() else { return };
        let path = glib::filename_from_uri(&dest).map(|(p, _)| p).unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !path.exists() {
            return; // failed or cancelled
        }
        util::log(&format!("download finished: {}", path.display()));
        let folder = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        popup::show(
            "download",
            vec![
                Item::heading("Download finished"),
                Item::text(name),
                Item::new(2, "Close"),
                Item::new(1, "Open folder").hot(true),
            ],
            ui::corner_anchor(),
            true,
            move |pick| {
                if pick == Some(1) {
                    util::open_path(&folder);
                }
            },
        );
    });
    dl.connect_failed(|_, e| util::log(&format!("download failed: {e}")));
}

// ---- Default browser ----------------------------------------------------------------------

fn apps_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_default();
    base.join("applications")
}

/// Writes the desktop entry (menu item) and the icon.
pub fn install_desktop_entry() {
    let icon = util::data_dir().join("suppa.svg");
    let _ = std::fs::write(&icon, crate::icon::svg());
    let exe = scripts::exe_path();
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Suppa Browser\nGenericName=Web Browser\nComment=Tiny, fast, private web browser\n\
         Exec=\"{}\" %U\nIcon={}\nTerminal=false\nCategories=Network;WebBrowser;\n\
         MimeType=text/html;application/xhtml+xml;x-scheme-handler/http;x-scheme-handler/https;\nStartupWMClass=suppa\n\
         Actions=new-private;\n\n[Desktop Action new-private]\nName=New incognito window\nExec=\"{}\" --window --incognito\n",
        exe.display(),
        icon.display(),
        exe.display()
    );
    let dir = apps_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("suppa.desktop"), entry);
    let _ = std::process::Command::new("update-desktop-database").arg(&dir).status();
}

pub fn is_default_browser() -> bool {
    std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "suppa.desktop")
        .unwrap_or(false)
}

fn make_default() {
    install_desktop_entry();
    let ok = std::process::Command::new("xdg-settings")
        .args(["set", "default-web-browser", "suppa.desktop"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    for mime in ["x-scheme-handler/http", "x-scheme-handler/https", "text/html"] {
        let _ = std::process::Command::new("xdg-mime").args(["default", "suppa.desktop", mime]).status();
    }
    popup::message(if ok {
        "Suppa is your default browser\n\nLinks from other programs now open in Suppa."
    } else {
        "Suppa could not be made the default browser\n\nChoose Suppa Browser as the web browser in your desktop's settings (Default Applications)."
    });
    crate::settings_page::broadcast();
}
