// Suppa Browser for Linux - the same browser as the Windows version, on GTK and
// WebKitGTK instead of Win32 and WebView2. The interface (scripts/ folder:
// bar, menus, settings page, cookie clicker) and the settings code are shared
// with the Windows version.
//
//   main.rs          entry point, command line, single instance
//   app.rs           shared state, tabs, command IDs
//   window.rs        main window, commands, shortcuts, downloads, session, Update
//   ui.rs            the bar (scripts/ui.html)
//   tabs.rs          tabs and their WebKit views
//   popup.rs, menu.rs  themed menus and dialogs (scripts/menu.html)
//   saved.rs         saved tabs bar        groups.rs   tab groups
//   settings_page.rs the Settings tab      adblock.rs  ad & tracker blocking
//   scripts.rs       the scripts folder, Update / rebuild
//   ../src/settings.rs  settings.ini + session (shared with Windows)
mod adblock;
mod app;
mod groups;
mod icon;
mod menu;
mod popup;
mod saved;
#[path = "../../src/settings.rs"]
#[allow(dead_code)] // shared with Windows: some parts are Windows-only
mod settings;
mod settings_page;
mod scripts;
mod tabs;
mod ui;
mod util;
mod window;

use gtk::prelude::*;

/// The WebKitGTK method traits (the crate has no prelude). Imported without
/// names: GTK has traits called SettingsExt / PrintOperationExt too.
mod wk {
    pub use javascriptcore::ValueExt as _;
    pub use webkit2gtk::{
        CookieManagerExt as _, DownloadExt as _, NavigationPolicyDecisionExt as _, PolicyDecisionExt as _,
        PrintOperationExt as _, ResponsePolicyDecisionExt as _, SecurityManagerExt as _, SettingsExt as _,
        URIRequestExt as _, URISchemeRequestExt as _, UserContentManagerExt as _, WebContextExt as _,
        WebInspectorExt as _, WebViewExt as _, WebsiteDataManagerExt as _, WebsiteDataManagerExtManual as _,
    };
}

const APP_ID: &str = "io.github.un1que0ff.Suppa";

fn main() {
    util::mark_start();
    // Window positions (menus, restored windows) need X11; on Wayland desktops
    // this runs through XWayland. Set SUPPA_WAYLAND=1 to use Wayland directly.
    if std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("SUPPA_WAYLAND").is_none() {
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
    // Our own options; everything else (links, files) goes to GTK.
    let mut secondary = false;
    let mut incognito = false;
    let mut rest = vec![std::env::args().next().unwrap_or_else(|| "suppa".into())];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--window" => secondary = true,
            "--incognito" => incognito = true,
            "--restart" => {
                if let Some(pid) = args.next() {
                    window::wait_for_previous(&pid);
                }
            }
            "--install-desktop" => {
                window::install_desktop_entry();
                return;
            }
            "--version" => {
                println!("Suppa Browser {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH);
                return;
            }
            _ => rest.push(a),
        }
    }
    gtk::glib::set_prgname(Some("suppa"));
    gtk::glib::set_application_name("Suppa Browser");

    // One Suppa: links opened from other programs become tabs in the running
    // window. Windows made by dragging a tab out are separate processes.
    let mut flags = gtk::gio::ApplicationFlags::HANDLES_OPEN;
    if secondary || incognito {
        flags |= gtk::gio::ApplicationFlags::NON_UNIQUE;
    }
    let app_ = gtk::Application::new(Some(APP_ID), flags);
    let launch_urls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let started = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        let started = started.clone();
        let urls = launch_urls.clone();
        app_.connect_activate(move |a| {
            if started.replace(true) {
                if let Some(w) = app::with(|s| s.window.clone()) {
                    w.present();
                }
                return;
            }
            scripts::ensure_defaults();
            window::build(a, window::Launch { urls: urls.borrow_mut().drain(..).collect(), secondary, incognito });
        });
    }
    {
        let started = started.clone();
        app_.connect_open(move |a, files, _| {
            let list: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
            if started.replace(true) {
                for u in list {
                    tabs::new_tab(Some(u), true);
                }
                if let Some(w) = app::with(|s| s.window.clone()) {
                    w.present();
                }
                return;
            }
            scripts::ensure_defaults();
            window::build(a, window::Launch { urls: list, secondary, incognito });
        });
    }
    app_.run_with_args(&rest);
}
