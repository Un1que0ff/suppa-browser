// Suppa Browser - tiny, fast Windows browser on top of WebView2 (Chromium).
//
// File map (edit only the file you need):
//   main.rs        entry point
//   app.rs         shared state, Tab struct, command / message IDs
//   window.rs      main window, message loop, commands, fullscreen, Update/restart
//   ui.rs          hosts the HTML top bar (scripts\ui.html), layout, frame, theme
//   scripts.rs     the "scripts" folder next to Suppa.exe (UI, user scripts,
//                  custom filters), change detection, restart
//   tabs.rs        open / close / switch / navigate tabs
//   webview.rs     WebView2 environment + per-tab browser setup and events
//   adblock.rs     ad & tracker blocking (filter lists, cosmetic CSS, scriptlets)
//   extensions.rs  Chrome extensions (Web Store install, unpacked, popup, remove)
//   sleeper.rs     background-tab sleeping / discarding to save CPU + RAM
//   shortcuts.rs   keyboard shortcuts
//   menu.rs        main menu + ad-block menu
//   settings.rs    settings.ini + session restore
//   newtab.rs      built-in new-tab page
//   util.rs        helpers (strings, paths, URL handling, downloads)
#![windows_subsystem = "windows"]

mod adblock;
mod app;
mod extensions;
mod menu;
mod backup;
mod default_browser;
mod extpopup;
mod groups;
mod import;
mod newtab;
mod popup;
mod saved;
mod scripts;
mod settings;
mod settings_page;
mod shortcuts;
mod sleeper;
mod tabs;
mod ui;
mod util;
mod webview;
mod window;

fn main() {
    util::mark_start();
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    window::run();
}
