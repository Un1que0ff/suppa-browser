// Main window: creation, message loop, command dispatcher, fullscreen, shutdown.
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::ClearBrowsingDataCompletedHandler;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Interface, w};

use crate::app::{self, *};
use crate::{adblock, extensions, menu, newtab, scripts, settings, shortcuts, sleeper, tabs, ui, util, webview};

thread_local! {
    /// Fingerprint of the scripts folder + exe when this copy started.
    static START_SIGNATURE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub fn run() {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("SuppaBrowserWindow");

        // Command line:
        //   --restart <pid>          started by the Update button (wait for the old copy)
        //   --window x,y,w,h <url>   a tab dragged out of a window (extra window)
        //   --incognito              that tab is incognito
        //   <url or file>            open this
        let args: Vec<String> = std::env::args().collect();
        let (mut restarted, mut restart_pid, mut new_window, mut incognito, mut arg_url) = (false, None, None, false, None);
        let mut it = args.iter().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--restart" => {
                    restarted = true;
                    restart_pid = it.next().and_then(|p| p.parse::<u32>().ok());
                }
                "--window" => {
                    let n: Vec<i32> = it.next().map(|r| r.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
                    new_window = Some(if n.len() == 4 { (n[0], n[1], n[2], n[3]) } else { (100, 100, 1200, 800) });
                }
                "--incognito" => incognito = true,
                s if !s.starts_with("--") && arg_url.is_none() => arg_url = Some(s.to_string()),
                _ => {}
            }
        }
        if let Some(pid) = restart_pid {
            scripts::wait_for_previous(pid);
        }
        let secondary = new_window.is_some();

        // Single instance: hand the URL to the running window and exit.
        if let (false, Ok(existing)) = (restarted || secondary, FindWindowW(class, None)) {
            if !existing.is_invalid() {
                let url = arg_url.clone().unwrap_or_default();
                let data: Vec<u16> = url.encode_utf16().collect();
                let cds = COPYDATASTRUCT { dwData: 0x5355, cbData: (data.len() * 2) as u32, lpData: data.as_ptr() as *mut _ };
                SendMessageW(existing, WM_COPYDATA, None, Some(LPARAM(&cds as *const _ as isize)));
                if IsIconic(existing).as_bool() {
                    let _ = ShowWindow(existing, SW_RESTORE);
                }
                let _ = SetForegroundWindow(existing);
                return;
            }
        }

        // Icon compiled into the exe by build.rs (from assets\icon.svg), resource id 1.
        let icon_id = windows::core::PCWSTR(1 as *const u16);
        let load_icon = |cx: i32, cy: i32| -> HICON {
            LoadImageW(Some(hinst.into()), icon_id, IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR)
                .map(|h| HICON(h.0))
                .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
        };
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: load_icon(GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON)),
            hIconSm: load_icon(GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)),
            // No class brush: ui::paint draws the themed background + frame.
            ..Default::default()
        };
        RegisterClassExW(&wc);

        scripts::remove_old_exe();
        // A backup chosen in "Restore" is put in place now, before anything reads the files.
        if !secondary {
            crate::backup::apply_pending();
        }
        scripts::ensure_defaults();
        START_SIGNATURE.with(|s| s.set(scripts::signature()));

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Suppa"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .expect("CreateWindowExW failed");
        // Re-run WM_NCCALCSIZE so the system title bar disappears (tabs take its place).
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
        crate::popup::init(hwnd);
        crate::extpopup::init(hwnd);

        let dpi = GetDpiForWindow(hwnd).max(96);
        let settings = settings::load();
        let dark = settings.dark;
        ui::apply_window_theme(hwnd, dark);
        app::init(State {
            hwnd,
            ui: ui::Ui::new(dark),
            env: None,
            browser_version: String::new(),
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            closed: Vec::new(),
            building: false,
            groups: Vec::new(),
            next_group: 1,
            secondary,
            settings,
            extensions: Vec::new(),
            extensions_loaded: false,
            fullscreen: None,
            dpi,
            minimized: false,
        });

        // Initial tabs: command-line URL, else restored session, else new tab page.
        // After an Update restart the tabs always come back.
        // An extra window (tab dragged out) starts with just that tab.
        // After a crash (the "running" marker of the last start is still there)
        // the tabs always come back, even if restoring is turned off.
        let marker = util::data_dir().join("running.lock");
        let crashed = !secondary && !restarted && marker.exists();
        if crashed {
            util::log("previous run did not close properly - reopening its tabs");
        }
        if !secondary {
            let _ = std::fs::write(&marker, std::process::id().to_string());
        }
        let restore = !secondary && (restarted || crashed || app::with(|s| s.settings.restore_session).unwrap_or(true));
        let (urls, active) = if restore { settings::load_session() } else { (Vec::new(), 0) };
        for u in &urls {
            tabs::add_lazy(u.clone());
        }
        app::with(|s| s.active = active);
        if restore {
            crate::groups::restore(&settings::load_session_meta());
        }
        if let Some(u) = arg_url {
            let url = app::with(|s| util::normalize_input(&u, s.settings.search_base())).unwrap_or(u);
            tabs::add_lazy(url);
            app::with(|s| {
                s.active = s.tabs.len() - 1;
                if let Some(t) = s.tabs.last_mut() {
                    t.incognito = incognito;
                }
            });
        }
        if app::with(|s| s.tabs.is_empty()).unwrap_or(true) {
            let home = app::with(|s| s.settings.home.clone()).unwrap_or_else(|| newtab::URL.to_string());
            tabs::add_lazy(home);
        }
        app::with(|s| {
            ui::select_tab_item(s, s.active);
            ui::refresh_toolbar(s);
        });

        // Same size, place and maximized state as last time.
        let (saved_rect, maximized) = app::with(|s| (s.settings.window, s.settings.maximized)).unwrap_or((None, false));
        let mut placed = false;
        if let Some((x, y, w, h)) = new_window {
            // Extra window: where the tab was dropped, same size as the window it came from.
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
            placed = true;
        } else if let Some((l, t, r, b)) = saved_rect {
            let rc = RECT { left: l, top: t, right: r, bottom: b };
            // Only if that place is still on a connected monitor.
            if !windows::Win32::Graphics::Gdi::MonitorFromRect(&rc, windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONULL).is_invalid() {
                let wp = WINDOWPLACEMENT {
                    length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    showCmd: if maximized { SW_SHOWMAXIMIZED.0 as u32 } else { SW_SHOWNORMAL.0 as u32 },
                    rcNormalPosition: rc,
                    ..Default::default()
                };
                placed = SetWindowPlacement(hwnd, &wp).is_ok();
            }
        }
        if !placed {
            let _ = ShowWindow(hwnd, if maximized { SW_SHOWMAXIMIZED } else { SW_SHOWDEFAULT });
        }
        ui::layout();
        SetTimer(Some(hwnd), TIMER_SLEEP, 30_000, None);
        // Files are watched by change notifications; TIMER_WATCH only runs once,
        // 400 ms after a change (several changes in a row = one check).
        scripts::start_watcher(hwnd);

        adblock::start(hwnd, false);
        webview::create_environment(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
                let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                let alt = GetKeyState(VK_MENU.0 as i32) < 0;
                if let Some(cmd) = shortcuts::map(msg.wParam.0 as u32, ctrl, shift, alt) {
                    command(cmd);
                    continue;
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_SIZE => {
                let minimized = wp.0 as u32 == SIZE_MINIMIZED;
                let ctrl = app::with(|s| {
                    s.minimized = minimized;
                    s.active_tab().and_then(|t| t.controller.clone())
                })
                .flatten();
                if let Some(c) = ctrl {
                    // A minimized browser does no rendering work at all.
                    let _ = c.SetIsVisible(!minimized);
                }
                ui::layout();
                ui::request_sync(); // maximize / restore button icon
                LRESULT(0)
            }
            WM_NCCALCSIZE if wp.0 != 0 => {
                // Keep the side/bottom resize borders but drop the title bar:
                // the tab row is drawn where the title bar was.
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                if style & WS_THICKFRAME.0 == 0 {
                    return DefWindowProcW(hwnd, msg, wp, lp);
                }
                let params = &mut *(lp.0 as *mut NCCALCSIZE_PARAMS);
                let top = params.rgrc[0].top;
                let r = DefWindowProcW(hwnd, msg, wp, lp);
                if r.0 != 0 {
                    return r;
                }
                params.rgrc[0].top = top;
                if IsZoomed(hwnd).as_bool() {
                    // A maximized window hangs over the screen edge by the frame size.
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
                    params.rgrc[0].top += GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                }
                LRESULT(0)
            }
            WM_NCHITTEST => {
                // Bar at the bottom: the thin strip at the top edge resizes the window.
                let r = DefWindowProcW(hwnd, msg, wp, lp);
                if r.0 as u32 == HTCLIENT {
                    let mut pt = windows::Win32::Foundation::POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
                    if ui::in_top_strip(pt.y) {
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        let corner = ui::sc(12, GetDpiForWindow(hwnd).max(96));
                        return LRESULT(if pt.x < corner {
                            HTTOPLEFT as isize
                        } else if pt.x >= rc.right - corner {
                            HTTOPRIGHT as isize
                        } else {
                            HTTOP as isize
                        });
                    }
                }
                r
            }
            WM_APP_NC_DRAG => {
                // Drag the window / resize its top edge from the HTML tab row.
                use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
                let _ = ReleaseCapture();
                let mut pt = windows::Win32::Foundation::POINT::default();
                let _ = GetCursorPos(&mut pt);
                let pos = ((pt.y as u32 & 0xFFFF) << 16 | (pt.x as u32 & 0xFFFF)) as isize;
                let ht = if wp.0 == HTCAPTION as usize || !IsZoomed(hwnd).as_bool() { wp.0 } else { 0 };
                if ht != 0 {
                    SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(ht)), Some(LPARAM(pos)));
                }
                LRESULT(0)
            }
            WM_APP_BUILD_DONE => {
                app::with(|s| s.building = false);
                ui::post_update();
                if wp.0 == 1 {
                    restart();
                } else {
                    crate::popup::confirm(
                        "Building the new Suppa.exe failed",
                        "Your Rust code has an error. Suppa keeps running the old version.",
                        "Open build log",
                        "Close",
                        || util::open_in_explorer(&scripts::build_log()),
                    );
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                ui::paint(hwnd);
                LRESULT(0)
            }
            WM_MOVE | WM_MOVING => {
                if let Some(Some(c)) = app::with(|s| s.active_tab().and_then(|t| t.controller.clone())) {
                    let _ = c.NotifyParentWindowPositionChanged();
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xFFFF) as u32;
                app::with(|s| ui::on_dpi_changed(s, dpi));
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                ui::layout();
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let mmi = &mut *(lp.0 as *mut MINMAXINFO);
                let dpi = app::with(|s| s.dpi).unwrap_or(96);
                mmi.ptMinTrackSize.x = ui::sc(480, dpi);
                mmi.ptMinTrackSize.y = ui::sc(300, dpi);
                LRESULT(0)
            }
            WM_TIMER => {
                if wp.0 == TIMER_SESSION {
                    let _ = KillTimer(Some(hwnd), TIMER_SESSION);
                    tabs::save_session_now();
                } else if wp.0 == TIMER_SLEEP {
                    sleeper::tick(false);
                } else if wp.0 == TIMER_WATCH {
                    // Light up "Update" when something in the scripts folder (or the exe) changed.
                    let _ = KillTimer(Some(hwnd), TIMER_WATCH);
                    let changed = scripts::signature() != START_SIGNATURE.with(|s| s.get());
                    ui::set_update_available(changed);
                }
                LRESULT(0)
            }
            // Windows accent colour changed (used when the accent is set to "Windows").
            WM_DWMCOLORIZATIONCOLORCHANGED => {
                if app::with(|s| s.settings.accent == "windows").unwrap_or(false) {
                    ui::push_theme();
                    crate::settings_page::broadcast();
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_APP_FILES_CHANGED => {
                SetTimer(Some(hwnd), TIMER_WATCH, 400, None);
                LRESULT(0)
            }
            WM_APP_UI_SYNC => {
                ui::sync_now();
                // Tabs changed: save them 2 s after the last change (crash safety).
                if !app::with(|s| s.secondary).unwrap_or(true) {
                    SetTimer(Some(hwnd), TIMER_SESSION, 2_000, None);
                }
                LRESULT(0)
            }
            WM_APP_CLOSE_TAB_INDEX => {
                tabs::close_tab(wp.0);
                LRESULT(0)
            }
            WM_ACTIVATE => {
                if wp.0 & 0xFFFF != 0 {
                    crate::saved::reload_if_changed();
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_SETFOCUS => {
                ui::focus_webview();
                LRESULT(0)
            }
            WM_COPYDATA => {
                // URL from a second launch (e.g. clicking a link in another app).
                let cds = &*(lp.0 as *const COPYDATASTRUCT);
                if cds.dwData == 0x5355 {
                    let slice = std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2);
                    let text = String::from_utf16_lossy(slice);
                    let url = app::with(|s| util::normalize_input(&text, s.settings.search_base())).unwrap_or_default();
                    tabs::new_tab(if url.is_empty() { None } else { Some(url) }, true);
                }
                LRESULT(1)
            }
            WM_APP_ADBLOCK_READY => {
                adblock::on_ready();
                app::with(|s| ui::refresh_toolbar(s));
                LRESULT(0)
            }
            WM_APP_EXT_UNPACKED => {
                extensions::on_unpacked(lp.0);
                LRESULT(0)
            }
            WM_APP_CLOSE_TAB_ID => {
                tabs::close_tab_id(wp.0 as u32);
                LRESULT(0)
            }
            WM_APP_FULLSCREEN => {
                set_fullscreen(wp.0 != 0);
                LRESULT(0)
            }
            WM_APP_COMMAND => {
                command(wp.0 as u16);
                LRESULT(0)
            }
            WM_CLOSE => {
                shutdown();
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Executes a toolbar / menu / shortcut command.
pub fn command(id: u16) {
    match id {
        CMD_UPDATE => do_update(),
        CMD_OPEN_LOG => {
            let log = util::data_dir().join("suppa.log");
            if !log.exists() {
                util::log("log created");
            }
            util::open_in_explorer(&log);
        }
        CMD_OPEN_SCRIPTS => util::open_in_explorer(scripts::dir()),
        CMD_SETTINGS => tabs::open_settings(),
        CMD_NEW_INCOGNITO => tabs::new_incognito(),
        CMD_TOGGLE_THEME => {
            app::with(|s| {
                s.settings.dark = !s.settings.dark;
                s.ui.dark = s.settings.dark;
                settings::save(&s.settings);
            });
            ui::push_theme();
            crate::settings_page::broadcast();
        }
        CMD_MINIMIZE => unsafe {
            let _ = ShowWindow(app::hwnd(), SW_MINIMIZE);
        },
        CMD_MAXIMIZE => unsafe {
            let h = app::hwnd();
            let _ = ShowWindow(h, if IsZoomed(h).as_bool() { SW_RESTORE } else { SW_MAXIMIZE });
        },
        CMD_CLOSE_WINDOW => util::post(app::hwnd(), WM_CLOSE, 0, 0),
        CMD_CLEAR_PASSWORDS => clear_passwords(),
        CMD_SAVED_RENAME => crate::saved::run_pending_rename(),
        CMD_MAKE_DEFAULT => crate::default_browser::make_default(),
        CMD_BACKUP => crate::backup::backup(),
        CMD_RESTORE => crate::backup::restore(),
        CMD_IMPORT_MENU => {
            let items = crate::import::menu_items();
            crate::popup::show("import", items, ui::corner_anchor(), true, |pick| {
                if let Some(id) = pick {
                    crate::import::run(id);
                }
            });
        }
        CMD_LOCK_SIZES => {
            app::with(|s| {
                s.settings.lock_sizes = !s.settings.lock_sizes;
                settings::save(&s.settings);
            });
            ui::push_theme();
        }
        CMD_NEW_TAB => {
            tabs::new_tab(None, true);
        }
        CMD_CLOSE_TAB => tabs::close_active(),
        CMD_REOPEN_TAB => tabs::reopen_closed(),
        CMD_CLOSE_OTHER_TABS => tabs::close_others(),
        CMD_DUPLICATE_TAB => tabs::duplicate_active(),
        CMD_NEXT_TAB => tabs::activate_relative(1),
        CMD_PREV_TAB => tabs::activate_relative(-1),
        CMD_BACK => tabs::go_back(),
        CMD_FORWARD => tabs::go_forward(),
        CMD_RELOAD => tabs::reload(),
        CMD_HARD_RELOAD => tabs::hard_reload(),
        CMD_HOME => tabs::go_home(),
        CMD_FOCUS_ADDRESS => ui::focus_address(),
        CMD_FULLSCREEN => {
            let on = app::with(|s| s.fullscreen.is_none()).unwrap_or(false);
            set_fullscreen(on);
        }
        CMD_ESCAPE => set_fullscreen(false),
        CMD_MENU => menu::show_main(),
        CMD_ADBLOCK_MENU => menu::show_adblock(),
        CMD_EXT_MENU => extensions::show_menu(),
        CMD_DOWNLOADS => tabs::downloads(),
        CMD_TASK_MANAGER => tabs::task_manager(),
        CMD_SLEEP_NOW => sleeper::tick(true),
        CMD_ZOOM_IN => tabs::zoom(0.1),
        CMD_ZOOM_OUT => tabs::zoom(-0.1),
        CMD_ZOOM_RESET => tabs::zoom(0.0),
        CMD_PRINT => tabs::print(),
        CMD_DEVTOOLS => tabs::devtools(),
        CMD_CLEAR_DATA => clear_data(),
        CMD_OPEN_DATA_DIR => util::open_in_explorer(&util::data_dir()),
        CMD_ABOUT => about(),
        CMD_TOGGLE_RESTORE => {
            app::with(|s| {
                s.settings.restore_session = !s.settings.restore_session;
                settings::save(&s.settings);
            });
        }
        CMD_TOGGLE_ADBLOCK => {
            app::with(|s| {
                s.settings.adblock = !s.settings.adblock;
                settings::save(&s.settings);
                ui::refresh_toolbar(s);
            });
            tabs::reload();
        }
        CMD_TOGGLE_SITE => {
            app::with(|s| {
                if let Some(host) = s.active_tab().map(|t| util::host_of(&t.doc_url)) {
                    s.settings.toggle_allow(&host);
                    settings::save(&s.settings);
                }
                ui::refresh_toolbar(s);
            });
            tabs::reload();
        }
        CMD_UPDATE_LISTS => {
            adblock::start(app::hwnd(), true);
            util::info(app::hwnd(), "Updating filter lists in the background.\nThey will be active in a few seconds.");
        }
        c if (CMD_SEARCH_BASE..CMD_SEARCH_BASE + 10).contains(&c) => {
            app::with(|s| {
                s.settings.search = (c - CMD_SEARCH_BASE) as usize;
                settings::save(&s.settings);
            });
        }
        c if (CMD_TAB_BASE..CMD_TAB_BASE + 9).contains(&c) => tabs::activate_number((c - CMD_TAB_BASE) as usize),
        c if c >= 2900 => extensions::command(c),
        _ => {}
    }
}

/// Borderless fullscreen (F11 or a video's fullscreen button).
pub fn set_fullscreen(on: bool) {
    let Some((hwnd, current)) = app::with(|s| (s.hwnd, s.fullscreen.is_some())) else { return };
    if on == current {
        return;
    }
    unsafe {
        if on {
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
            let _ = GetWindowPlacement(hwnd, &mut wp);
            app::with(|s| s.fullscreen = Some((style, wp)));
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            SetWindowLongW(hwnd, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW.0 as i32));
            let r = mi.rcMonitor;
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED | SWP_NOOWNERZORDER);
        } else {
            let saved = app::with(|s| s.fullscreen.take()).flatten();
            if let Some((style, wp)) = saved {
                SetWindowLongW(hwnd, GWL_STYLE, style);
                let _ = SetWindowPlacement(hwnd, &wp);
                let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED);
            }
        }
    }
    ui::layout();
}

fn clear_data() {
    crate::popup::confirm(
        "Delete all browsing data?",
        "History, cookies, cache, saved passwords and site data are deleted. You will be signed out of websites.",
        "Delete",
        "Cancel",
        clear_data_now,
    );
}

fn clear_data_now() {
    let hwnd = app::hwnd();
    tabs::with_active_webview(|wv| unsafe {
        if let Ok(p) = wv.cast::<ICoreWebView2_13>().and_then(|w| w.Profile()).and_then(|p| p.cast::<ICoreWebView2Profile2>()) {
            let _ = p.ClearBrowsingDataAll(&ClearBrowsingDataCompletedHandler::create(Box::new(move |_| {
                util::info(hwnd, "Browsing data cleared.");
                Ok(())
            })));
        }
    });
}

fn clear_passwords() {
    crate::popup::confirm("Delete all saved passwords?", "This cannot be undone.", "Delete", "Cancel", clear_passwords_now);
}

fn clear_passwords_now() {
    let hwnd = app::hwnd();
    let wv = app::with(|s| s.tabs.iter().filter(|t| !t.incognito).find_map(|t| t.webview.clone())).flatten();
    let Some(wv) = wv else {
        util::info(hwnd, "Open a normal (not incognito) tab first.");
        return;
    };
    unsafe {
        if let Ok(p) = wv.cast::<ICoreWebView2_13>().and_then(|w| w.Profile()).and_then(|p| p.cast::<ICoreWebView2Profile2>()) {
            let _ = p.ClearBrowsingData(
                COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE,
                &ClearBrowsingDataCompletedHandler::create(Box::new(move |_| {
                    util::info(hwnd, "Saved passwords deleted.");
                    Ok(())
                })),
            );
        }
    }
}

fn about() {
    let r = app::with(|s| (s.hwnd, s.browser_version.clone(), s.tabs.len()));
    let Some((hwnd, ver, n)) = r else { return };
    util::info(
        hwnd,
        &format!(
            "About Suppa Browser\n\nVersion: {}\nPlatform: Windows (64-bit)\nEngine: Microsoft Edge WebView2 {}\nAd blocker: {}\nOpen tabs: {}\nData folder: {}\n\nhttps://github.com/Un1que0ff/suppa-browser",
            env!("CARGO_PKG_VERSION"),
            if ver.is_empty() { "(starting)" } else { &ver },
            if adblock::is_ready() { "active" } else { "loading" },
            n,
            util::data_dir().display()
        ),
    );
}

/// Update button: save tabs, close, and start again so every file in the
/// scripts folder (and a rebuilt Suppa.exe) is loaded fresh.
/// If the Rust code (src\, Cargo.toml, assets\...) changed, it is rebuilt
/// first (in the background, Suppa keeps working); then Suppa restarts.
fn do_update() {
    let hwnd = app::hwnd();
    if app::with(|s| s.building).unwrap_or(false) {
        return;
    }
    let changed = app::with(|s| s.ui.update_available).unwrap_or(false);
    let rust = scripts::rust_changed();
    let (title, text, yes) = if rust {
        (
            "Build the new version of Suppa?",
            "The Rust code has changed. You can keep browsing while it builds; then Suppa restarts and reopens your tabs.",
            "Build and restart",
        )
    } else if changed {
        ("Load the changes?", "Files in the scripts folder have changed. Suppa restarts and reopens your tabs.", "Restart")
    } else {
        ("Restart Suppa?", "Everything is reloaded from the scripts folder. Your tabs are reopened.", "Restart")
    };
    crate::popup::confirm(title, text, yes, "Cancel", move || {
        if rust {
            app::with(|s| s.building = true);
            ui::post_update();
            scripts::start_build(hwnd);
        } else {
            restart();
        }
    });
}

/// Restart without asking (e.g. after choosing a backup to restore).
pub fn restart_now() {
    restart();
}

/// Saves the tabs, closes and starts again.
fn restart() {
    let hwnd = app::hwnd();
    shutdown();
    if let Err(e) = scripts::spawn_restart() {
        util::message_box(hwnd, &format!("Could not restart: {e}"), MB_ICONERROR); // window is closing: a native box is safest here
        return;
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

fn shutdown() {
    // An extra window only closes its tabs; the main window keeps the session,
    // window position and settings.
    if app::with(|s| s.secondary).unwrap_or(false) {
        let ctrls = app::with(|s| s.tabs.iter_mut().filter_map(|t| t.controller.take()).collect::<Vec<_>>()).unwrap_or_default();
        for c in ctrls {
            let _ = unsafe { c.Close() };
        }
        return;
    }
    let (urls, active, meta) = tabs::session_urls();
    let ctrls = app::with(|s| {
        // Remember the window (the size before full screen, if in full screen).
        let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        let have = match &s.fullscreen {
            Some((_, saved)) => {
                wp = *saved;
                true
            }
            None => unsafe { GetWindowPlacement(s.hwnd, &mut wp).is_ok() },
        };
        if have {
            let r = wp.rcNormalPosition;
            s.settings.window = Some((r.left, r.top, r.right, r.bottom));
            s.settings.maximized = wp.showCmd == SW_SHOWMAXIMIZED.0 as u32
                || (wp.showCmd == SW_SHOWMINIMIZED.0 as u32 && (wp.flags.0 & WPF_RESTORETOMAXIMIZED.0) != 0);
        }
        settings::save(&s.settings);
        s.tabs.iter_mut().filter_map(|t| t.controller.take()).collect::<Vec<_>>()
    })
    .unwrap_or_default();
    settings::save_session(&urls, active, &meta);
    // Closed normally: no crash recovery needed next time.
    let _ = std::fs::remove_file(util::data_dir().join("running.lock"));
    for c in ctrls {
        let _ = unsafe { c.Close() };
    }
}
