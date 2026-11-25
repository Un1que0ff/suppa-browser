// WebView2 environment and per-tab browser setup + event wiring.
use std::time::Instant;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetKeyState, VK_CONTROL, VK_MBUTTON, VK_MENU, VK_SHIFT};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR;
use windows::core::{BOOL, HSTRING, Interface, PCWSTR, PWSTR, w};

use crate::app::{self, *};
use crate::{adblock, extensions, newtab, popup, shortcuts, tabs, ui, util};

/// Extra Chromium switches: turn off Edge-only extras we don't need, and use
/// every CPU core / the GPU for drawing pages (Chromium already runs each site,
/// the GPU and the network in their own processes, spread over all cores):
///  * num-raster-threads=4: pages are drawn by 4 threads in parallel (Chromium's maximum)
///  * gpu-rasterization + zero-copy: drawing goes to the graphics card
///  * ParallelDownloading: big downloads use several connections at once
///
/// Less RAM / background work:
///  * process-per-site: all tabs of one site share one process (big RAM saving
///    with several tabs of the same site)
///  * no-pings: no <a ping> tracking requests
///  * Chromium background services Suppa doesn't use are off (translate,
///    optimization hints, cast/media router, domain reliability reports)
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,msEdgeShoppingUI,msEdgeEnhanceImagesEnabled,msEdgeTranslate,\
Translate,OptimizationHints,OptimizationGuideModelDownloading,MediaRouter,DialMediaRouteProvider,InterestFeedContentSuggestions \
--enable-features=BackForwardCache,ParallelDownloading \
--num-raster-threads=4 --enable-gpu-rasterization --enable-zero-copy \
--process-per-site --no-pings --disable-domain-reliability";

pub fn create_environment(hwnd: HWND) {
    let options = CoreWebView2EnvironmentOptions::default();
    let options: ICoreWebView2EnvironmentOptions = options.into();
    unsafe {
        let _ = options.SetAdditionalBrowserArguments(&HSTRING::from(BROWSER_ARGS));
        if let Ok(o6) = options.cast::<ICoreWebView2EnvironmentOptions6>() {
            let _ = o6.SetAreBrowserExtensionsEnabled(true);
        }
    }
    let user_data = util::sub_dir("UserData");
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |res, env| {
        match (res, env) {
            (Ok(()), Some(env)) => on_environment(env),
            (r, _) => {
                let msg = format!(
                    "Could not start the WebView2 engine ({:?}).\n\nInstall the Microsoft Edge WebView2 Runtime from\nhttps://go.microsoft.com/fwlink/p/?LinkId=2124703",
                    r.err()
                );
                util::message_box(hwnd, &msg, MB_ICONERROR);
            }
        }
        Ok(())
    }));
    unsafe {
        if let Err(e) = CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(user_data.as_os_str()),
            &options,
            &handler,
        ) {
            util::message_box(hwnd, &format!("WebView2 is not available: {e}"), MB_ICONERROR);
        }
    }
}

fn on_environment(env: ICoreWebView2Environment) {
    let mut ver = PWSTR::null();
    unsafe {
        let _ = env.BrowserVersionString(&mut ver);
    }
    let ver = util::take_pwstr(ver);
    let hwnd = app::with(|s| {
        s.env = Some(env.clone());
        s.browser_version = ver.split(' ').next().unwrap_or("").to_string();
        s.hwnd
    })
    .unwrap_or_default();
    // Top bar (scripts\ui.html) first, then the menu popup, then the page.
    ui::create(&env, hwnd);
    crate::popup::create(&env);
    // Load the active tab (others stay lazy).
    if let Some(a) = app::with(|s| s.active) {
        tabs::activate(a);
    }
}

pub fn set_memory_low(c: &ICoreWebView2Controller, low: bool) {
    unsafe {
        if let Ok(wv) = c.CoreWebView2() {
            if let Ok(w19) = wv.cast::<ICoreWebView2_19>() {
                let _ = w19.SetMemoryUsageTargetLevel(if low {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                });
            }
        }
    }
}

/// Starts creating the WebView for tab `id` (async).
pub fn create(id: u32) {
    let r = app::with(|s| {
        let env = s.env.clone()?;
        let hwnd = s.hwnd;
        let t = s.tab_mut(id)?;
        if t.creating || t.controller.is_some() {
            return None;
        }
        t.creating = true;
        let inc = t.incognito;
        ui::update_tab_by_id(s, id);
        Some((env, hwnd, inc))
    })
    .flatten();
    let Some((env, hwnd, incognito)) = r else { return };
    // A new page view starts in the theme's background colour instead of white
    // (no white flash when opening or waking a tab). WebView2 reads this when the
    // view is created; setup() switches to white once the page's content loads.
    let bg = app::with(|s| s.ui.bg.0).unwrap_or(0x161616);
    let argb = format!("FF{:02X}{:02X}{:02X}", bg & 255, (bg >> 8) & 255, (bg >> 16) & 255);
    unsafe {
        std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", argb);
    }
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |res, ctrl| {
        match (res, ctrl) {
            (Ok(()), Some(c)) => setup(id, c),
            _ => {
                app::with(|s| {
                    if let Some(t) = s.tab_mut(id) {
                        t.creating = false;
                    }
                });
            }
        }
        Ok(())
    }));
    unsafe {
        // Incognito: an InPrivate profile (cookies, history, cache are thrown away).
        let started = if incognito {
            match env.cast::<ICoreWebView2Environment10>().and_then(|e10| {
                let opts = e10.CreateCoreWebView2ControllerOptions()?;
                opts.SetProfileName(w!("Incognito"))?;
                opts.SetIsInPrivateModeEnabled(true)?;
                e10.CreateCoreWebView2ControllerWithOptions(hwnd, &opts, &handler)
            }) {
                Ok(()) => Ok(()),
                Err(e) => {
                    util::log(&format!("incognito tab failed: {e}"));
                    Err(e)
                }
            }
        } else {
            env.CreateCoreWebView2Controller(hwnd, &handler)
        };
        if started.is_err() {
            app::with(|s| {
                if let Some(t) = s.tab_mut(id) {
                    t.creating = false;
                }
            });
        }
    }
}

pub fn get_str(f: impl FnOnce(&mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut p = PWSTR::null();
    if f(&mut p).is_ok() { util::take_pwstr(p) } else { String::new() }
}

fn setup(id: u32, controller: ICoreWebView2Controller) {
    let Ok(wv) = (unsafe { controller.CoreWebView2() }) else { return };
    configure(&wv);
    // After restoring a backup: cookies (website logins) go back in now.
    if !app::with(|s| s.tab(id).map(|t| t.incognito).unwrap_or(false)).unwrap_or(false) {
        crate::backup::import_pending_cookies(&wv);
    }
    // Registered before the first Navigate, so it runs on the very first page too.
    if app::with(|s| s.settings.auto_cookies).unwrap_or(false) {
        add_cookie_script(id, &wv);
    }
    register_events(id, &controller, &wv);
    // Theme colour until the first page starts drawing, then the normal white base
    // (pages without a background of their own look as in any browser).
    unsafe {
        let bg = app::with(|s| s.ui.bg.0).unwrap_or(0x161616);
        if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: bg as u8, G: (bg >> 8) as u8, B: (bg >> 16) as u8 });
            let done = std::rc::Rc::new(std::cell::Cell::new(false));
            let mut token = 0i64;
            let _ = wv.add_ContentLoading(
                &ContentLoadingEventHandler::create(Box::new(move |_, _| {
                    if !done.replace(true) {
                        let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: 255, G: 255, B: 255 });
                    }
                    Ok(())
                })),
                &mut token,
            );
        }
    }

    let r = app::with(|s| {
        let rect = ui::content_rect(s);
        let active = s.is_active(id);
        let minimized = s.minimized;
        let t = s.tab_mut(id)?;
        t.controller = Some(controller.clone());
        t.webview = Some(wv.clone());
        t.creating = false;
        t.last_active = Instant::now();
        let pending = t.pending_new_window.take();
        let url = t.url.clone();
        let focus_addr = std::mem::take(&mut t.focus_address);
        ui::update_tab_by_id(s, id);
        Some((rect, active && !minimized, pending, url, focus_addr))
    })
    .flatten();
    // A new page window can land anywhere in the stack: the bar goes back under
    // all pages, the floating window buttons above them.
    ui::restack();

    let Some((rect, visible, pending, url, focus_addr)) = r else {
        // Tab was closed while the WebView was being created.
        let _ = unsafe { controller.Close() };
        return;
    };
    unsafe {
        let _ = controller.SetBounds(rect);
        let _ = controller.SetIsVisible(visible);
        set_memory_low(&controller, !visible);
        match pending {
            Some((args, deferral)) => {
                let _ = args.SetNewWindow(&wv);
                let _ = args.SetHandled(true);
                let _ = deferral.Complete();
            }
            None => {
                let _ = wv.Navigate(&HSTRING::from(url.as_str()));
            }
        }
        if visible && focus_addr {
            ui::focus_address();
        } else if visible && !newtab::is_newtab(&url) {
            let _ = controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
    }
    extensions::on_webview_ready(&wv);
}

fn add_cookie_script(id: u32, wv: &ICoreWebView2) {
    unsafe {
        let _ = wv.AddScriptToExecuteOnDocumentCreated(
            &HSTRING::from(crate::scripts::cookie_script()),
            &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |res, sid| {
                if res.is_ok() {
                    app::with(|s| {
                        if let Some(t) = s.tab_mut(id) {
                            t.cookie_script_id = Some(sid);
                        }
                    });
                }
                Ok(())
            })),
        );
    }
}

/// Turns the cookie-banner clicker on/off in every open tab (from Settings).
pub fn apply_cookie_setting() {
    let (on, tabs) = app::with(|s| {
        let tabs: Vec<(u32, ICoreWebView2, Option<String>)> =
            s.tabs.iter().filter_map(|t| Some((t.id, t.webview.clone()?, t.cookie_script_id.clone()))).collect();
        (s.settings.auto_cookies, tabs)
    })
    .unwrap_or((false, Vec::new()));
    for (id, wv, sid) in tabs {
        match (on, sid) {
            (true, None) => add_cookie_script(id, &wv),
            (false, Some(sid)) => {
                unsafe {
                    let _ = wv.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(sid));
                }
                app::with(|s| {
                    if let Some(t) = s.tab_mut(id) {
                        t.cookie_script_id = None;
                    }
                });
            }
            _ => {}
        }
    }
}

/// Password saving + form autofill, from the settings.
pub fn apply_autofill(wv: &ICoreWebView2) {
    let (pw, fill) = app::with(|s| (s.settings.save_passwords, s.settings.autofill)).unwrap_or((true, true));
    unsafe {
        if let Ok(s4) = wv.Settings().and_then(|st| st.cast::<ICoreWebView2Settings4>()) {
            let _ = s4.SetIsPasswordAutosaveEnabled(pw);
            let _ = s4.SetIsGeneralAutofillEnabled(fill);
        }
    }
}

fn configure(wv: &ICoreWebView2) {
    unsafe {
        if let Ok(st) = wv.Settings() {
            let _ = st.SetIsStatusBarEnabled(true);
            let _ = st.SetAreDevToolsEnabled(true);
            let _ = st.SetIsZoomControlEnabled(true);
            let _ = st.SetAreDefaultContextMenusEnabled(true);
            let _ = st.SetIsWebMessageEnabled(true);
        }
        apply_autofill(wv);
        // Route every request (incl. iframes + service workers) through the ad blocker.
        let added = wv.cast::<ICoreWebView2_22>().and_then(|w22| {
            w22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )
        });
        if added.is_err() {
            let _ = wv.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL);
        }
        let _ = wv.AddScriptToExecuteOnDocumentCreated(
            &HSTRING::from(adblock::COLLECTOR_SCRIPT),
            None::<&ICoreWebView2AddScriptToExecuteOnDocumentCreatedCompletedHandler>,
        );
        // Chrome Web Store: "Add to Suppa" uses Suppa's own installer (scripts\webstore.js).
        let _ = wv.AddScriptToExecuteOnDocumentCreated(
            &HSTRING::from(crate::scripts::read_text("webstore.js")),
            None::<&ICoreWebView2AddScriptToExecuteOnDocumentCreatedCompletedHandler>,
        );
        // Your own scripts from scripts\userscripts\*.js
        for script in crate::scripts::user_scripts() {
            let _ = wv.AddScriptToExecuteOnDocumentCreated(
                &HSTRING::from(script),
                None::<&ICoreWebView2AddScriptToExecuteOnDocumentCreatedCompletedHandler>,
            );
        }
    }
}

/// Keyboard shortcuts while a page (or the top bar) has focus.
pub fn accelerator_handler() -> ICoreWebView2AcceleratorKeyPressedEventHandler {
    AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        unsafe {
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            let _ = args.KeyEventKind(&mut kind);
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN {
                return Ok(());
            }
            let mut vk = 0u32;
            let _ = args.VirtualKey(&mut vk);
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
            let alt = GetKeyState(VK_MENU.0 as i32) < 0;
            if let Some(cmd) = shortcuts::map(vk, ctrl, shift, alt) {
                let _ = args.SetHandled(true);
                util::post(app::hwnd(), WM_APP_COMMAND, cmd as usize, 0);
            }
        }
        Ok(())
    }))
}

fn adblock_active_for(s: &State, doc_url: &str) -> bool {
    s.settings.adblock && !s.settings.is_allowed(&util::host_of(doc_url))
}

fn register_events(id: u32, controller: &ICoreWebView2Controller, wv: &ICoreWebView2) {
    let mut token = 0i64;
    unsafe {
        // Navigation starting: loading state + per-site ad-block script.
        let _ = wv.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let uri = get_str(|p| args.Uri(p));
                let r = app::with(|s| {
                    let enabled = adblock_active_for(s, &uri);
                    let t = s.tab_mut(id)?;
                    t.loading = true;
                    t.doc_url = uri.clone();
                    t.blocked = 0;
                    t.suspended = false;
                    let old = t.doc_script_id.take();
                    ui::update_tab_by_id(s, id);
                    Some((enabled, old))
                })
                .flatten();
                let Some((enabled, old)) = r else { return Ok(()) };
                if let Some(old) = old {
                    let _ = sender.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(old));
                }
                // Per-page script: ad-block cosmetics/scriptlets.
                let script = if enabled { adblock::document_script(&uri) } else { String::new() };
                {
                    if !script.trim().is_empty() {
                        let _ = sender.AddScriptToExecuteOnDocumentCreated(
                            &HSTRING::from(script),
                            &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |res, sid| {
                                if res.is_ok() {
                                    app::with(|s| {
                                        if let Some(t) = s.tab_mut(id) {
                                            t.doc_script_id = Some(sid);
                                        }
                                    });
                                }
                                Ok(())
                            })),
                        );
                    }
                }
                Ok(())
            })),
            &mut token,
        );

        let _ = wv.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |sender, _| {
                let Some(sender) = sender else { return Ok(()) };
                let url = get_str(|p| sender.Source(p));
                let record = app::with(|s| {
                    let mut rec = None;
                    if let Some(t) = s.tab_mut(id) {
                        if !t.incognito && t.url != url {
                            rec = Some(t.title.clone());
                        }
                        t.url = url.clone();
                    }
                    ui::update_tab_by_id(s, id);
                    rec.filter(|_| !s.secondary)
                })
                .flatten();
                if record.is_some() {
                    // The title still belongs to the previous page; it follows below.
                    crate::history::record(&url, "");
                }
                Ok(())
            })),
            &mut token,
        );

        let _ = wv.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |sender, _| {
                let Some(sender) = sender else { return Ok(()) };
                let title = get_str(|p| sender.DocumentTitle(p));
                let page = app::with(|s| {
                    let mut page = None;
                    if let Some(t) = s.tab_mut(id) {
                        t.title = title.clone();
                        if !t.incognito {
                            page = Some(t.url.clone());
                        }
                    }
                    ui::update_tab_by_id(s, id);
                    page.filter(|_| !s.secondary)
                })
                .flatten();
                if let Some(url) = page {
                    crate::history::set_title(&url, &title);
                }
                Ok(())
            })),
            &mut token,
        );

        let _ = wv.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |_, _| {
                app::with(|s| {
                    if s.is_active(id) {
                        ui::refresh_toolbar(s);
                    }
                });
                Ok(())
            })),
            &mut token,
        );

        let _ = wv.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, _| {
                app::with(|s| {
                    if let Some(t) = s.tab_mut(id) {
                        t.loading = false;
                    }
                    ui::update_tab_by_id(s, id);
                });
                Ok(())
            })),
            &mut token,
        );

        // Popups / target=_blank / Ctrl+click -> new tab.
        let _ = wv.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let uri = get_str(|p| args.Uri(p));
                let deferral = args.GetDeferral()?;
                let background = GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 || GetAsyncKeyState(VK_MBUTTON.0 as i32) < 0;
                let incognito = app::with(|s| s.tab(id).map(|t| t.incognito)).flatten().unwrap_or(false);
                tabs::new_tab_for_window(uri, args, deferral, background, incognito);
                Ok(())
            })),
            &mut token,
        );

        let _ = wv.add_WindowCloseRequested(
            &WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
                util::post(app::hwnd(), WM_APP_CLOSE_TAB_ID, id as usize, 0);
                Ok(())
            })),
            &mut token,
        );

        let _ = wv.add_ContainsFullScreenElementChanged(
            &ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
                let Some(sender) = sender else { return Ok(()) };
                let mut full = BOOL(0);
                let _ = sender.ContainsFullScreenElement(&mut full);
                util::post(app::hwnd(), WM_APP_FULLSCREEN, full.as_bool() as usize, 0);
                Ok(())
            })),
            &mut token,
        );

        // Network ad/tracker blocking.
        let _ = wv.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                on_resource_requested(id, &args);
                Ok(())
            })),
            &mut token,
        );

        // Downloads: our own list and popup instead of the engine's box.
        if let Ok(w4) = wv.cast::<ICoreWebView2_4>() {
            let _ = w4.add_DownloadStarting(
                &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let incognito = app::with(|s| s.tab(id).map(|t| t.incognito)).flatten().unwrap_or(false);
                    crate::downloads::on_starting(&args, incognito);
                    Ok(())
                })),
                &mut token,
            );
        }

        // Tab icon.
        if let Ok(w15) = wv.cast::<ICoreWebView2_15>() {
            let _ = w15.add_FaviconChanged(
                &FaviconChangedEventHandler::create(Box::new(move |sender, _| {
                    let Some(sender) = sender else { return Ok(()) };
                    let Ok(w15) = sender.cast::<ICoreWebView2_15>() else { return Ok(()) };
                    let fav = get_str(|p| w15.FaviconUri(p));
                    app::with(|s| {
                        if let Some(t) = s.tab_mut(id) {
                            t.favicon = fav;
                        }
                        ui::update_tab_by_id(s, id);
                    });
                    Ok(())
                })),
                &mut token,
            );
        }

        // Messages from pages: the settings page, and generic cosmetic
        // filtering (class/id list from the collector script).
        let _ = wv.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let url = get_str(|p| args.Source(p));
                if util::host_of(&url) == tabs::SETTINGS_HOST && url.starts_with("https://") {
                    let json = get_str(|p| args.WebMessageAsJson(p));
                    crate::settings_page::on_message(&sender, &json);
                    return Ok(());
                }
                let host = util::host_of(&url);
                if crate::pages::is_page(&host) && url.starts_with("https://") {
                    let json = get_str(|p| args.WebMessageAsJson(p));
                    crate::pages::on_message(&sender, &host, &json);
                    return Ok(());
                }
                let msg = get_str(|p| args.TryGetWebMessageAsString(p));
                // "Add to Suppa" on the Chrome Web Store (only trusted from that site).
                if let Some(rest) = msg.strip_prefix("suppa-ext-install:") {
                    if util::host_of(&url) == "chromewebstore.google.com" && url.starts_with("https://") {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                            let css = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
                            let dpi = app::with(|s| s.dpi).unwrap_or(96) as f64 / 96.0;
                            let anchor = page_to_screen(windows::Win32::Foundation::POINT {
                                x: (css("x") * dpi) as i32,
                                y: (css("y") * dpi) as i32,
                            });
                            extensions::ask_install(id, name, anchor);
                        }
                    }
                    return Ok(());
                }
                if !msg.contains("suppa-cos") {
                    return Ok(());
                }
                let enabled = app::with(|s| adblock_active_for(s, &url)).unwrap_or(false);
                if enabled {
                    let js = adblock::generic_hide_script(&url, &msg);
                    if !js.is_empty() {
                        let _ = sender.ExecuteScript(&HSTRING::from(js), None::<&ICoreWebView2ExecuteScriptCompletedHandler>);
                    }
                }
                Ok(())
            })),
            &mut token,
        );

        // Right-click menu on pages: same items as Edge's, drawn by the themed popup.
        if let Ok(w11) = wv.cast::<ICoreWebView2_11>() {
            let _ = w11.add_ContextMenuRequested(
                &ContextMenuRequestedEventHandler::create(Box::new(move |sender, args| {
                    let Some(args) = args else { return Ok(()) };
                    let Ok(coll) = args.MenuItems() else { return Ok(()) };
                    let mut items = context_items(&coll);
                    // "Inspect element" is always there: Edge's own item (it selects the
                    // clicked element in the developer tools), or ours if Edge left it out.
                    let has_inspect = items.iter().any(|i| i.label == "Inspect element");
                    if !has_inspect {
                        if !items.is_empty() {
                            items.push(popup::Item::sep());
                        }
                        items.push(popup::Item::new(OUR_INSPECT, "Inspect element").key("F12"));
                    }
                    let mut pt = windows::Win32::Foundation::POINT::default();
                    let _ = args.Location(&mut pt);
                    let deferral = args.GetDeferral()?;
                    args.SetHandled(true)?;
                    let anchor = page_to_screen(pt);
                    let wv_for_tools = sender.clone();
                    popup::show("page", items, anchor, false, move |pick| {
                        match pick {
                            Some(OUR_INSPECT) => {
                                if let Some(w) = &wv_for_tools {
                                    let _ = w.OpenDevToolsWindow();
                                }
                            }
                            Some(cmd) => {
                                let _ = args.SetSelectedCommandId(cmd as i32);
                            }
                            None => {}
                        }
                        let _ = deferral.Complete();
                    });
                    Ok(())
                })),
                &mut token,
            );
        }

        // Crashed page -> reload it.
        let _ = wv.add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                let _ = args.ProcessFailedKind(&mut kind);
                if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                    let _ = sender.Reload();
                }
                Ok(())
            })),
            &mut token,
        );

        // Keyboard shortcuts while the page has focus.
        let _ = controller.add_AcceleratorKeyPressed(&accelerator_handler(), &mut token);
    }
}

/// Our own "Inspect element" item (when Edge's menu has none).
const OUR_INSPECT: i64 = 900_000_001;

/// Edge's context-menu items -> themed popup items (same command ids).
fn context_items(coll: &ICoreWebView2ContextMenuItemCollection) -> Vec<popup::Item> {
    let mut out = Vec::new();
    unsafe {
        let mut n = 0u32;
        let _ = coll.Count(&mut n);
        for i in 0..n {
            let Ok(it) = coll.GetValueAtIndex(i) else { continue };
            let mut kind = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND::default();
            let _ = it.Kind(&mut kind);
            if kind == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR {
                if out.last().map(|x: &popup::Item| !x.sep).unwrap_or(false) {
                    out.push(popup::Item::sep());
                }
                continue;
            }
            // "&Copy" -> "Copy" ("&&" stays "&").
            let label = get_str(|p| it.Label(p)).replace("&&", "\u{1}").replace('&', "").replace('\u{1}', "&");
            // Edge calls it "Inspect"; it opens the developer tools on the clicked element.
            let label = if get_str(|p| it.Name(p)) == "inspectElement" || label == "Inspect" { "Inspect element".to_string() } else { label };
            let mut id = 0i32;
            let _ = it.CommandId(&mut id);
            let (mut en, mut ch) = (BOOL(0), BOOL(0));
            let _ = it.IsEnabled(&mut en);
            let _ = it.IsChecked(&mut ch);
            let key = get_str(|p| it.ShortcutKeyDescription(p));
            if kind == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SUBMENU {
                let children = it.Children().map(|c| context_items(&c)).unwrap_or_default();
                out.push(popup::Item::sub(label, children).enabled(en.as_bool()));
            } else {
                out.push(popup::Item::new(id as i64, label).key(&key).enabled(en.as_bool()).checked(ch.as_bool()));
            }
        }
    }
    while out.last().map(|x| x.sep).unwrap_or(false) {
        out.pop();
    }
    out
}

/// Point inside the page (WebView pixels) -> screen pixels.
fn page_to_screen(pt: windows::Win32::Foundation::POINT) -> windows::Win32::Foundation::POINT {
    let r = app::with(|s| (ui::content_rect(s), s.hwnd));
    let Some((rc, hwnd)) = r else { return pt };
    let mut p = windows::Win32::Foundation::POINT { x: rc.left + pt.x, y: rc.top + pt.y };
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut p);
    }
    p
}

fn on_resource_requested(id: u32, args: &ICoreWebView2WebResourceRequestedEventArgs) {
    unsafe {
        let Ok(req) = args.Request() else { return };
        let uri = get_str(|p| req.Uri(p));
        let mut ctx = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
        let _ = args.ResourceContext(&mut ctx);
        let r = app::with(|s| {
            let t = s.tab(id)?;
            let enabled = adblock_active_for(s, &t.doc_url);
            Some((t.doc_url.clone(), s.env.clone()?, enabled, s.settings.search_base()))
        })
        .flatten();
        let Some((doc_url, env, enabled, search)) = r else { return };
        // Settings page (scripts\settings.html and files next to it).
        if util::host_of(&uri) == tabs::SETTINGS_HOST {
            let path = uri.split_once("://").and_then(|(_, r)| r.split_once('/')).map(|(_, p)| p).unwrap_or("");
            let path = path.split(['?', '#']).next().unwrap_or("");
            let file = if path.is_empty() { "settings.html" } else { path };
            let (status, mime, body) = match crate::scripts::serve(&format!("https://x/{file}")) {
                Some((m, b)) => (200, m, b),
                None => (404, "text/plain", b"not found".to_vec()),
            };
            let stream = SHCreateMemStream(Some(&body));
            if let Ok(resp) = env.CreateWebResourceResponse(
                stream.as_ref(),
                status,
                w!("OK"),
                &HSTRING::from(format!("Content-Type: {mime}\r\nCache-Control: no-store")),
            ) {
                let _ = args.SetResponse(&resp);
            }
            return;
        }
        // History and Downloads pages (scripts\history.html, downloads.html).
        if let Some(file) = crate::pages::file_for(&util::host_of(&uri)) {
            let path = uri.split_once("://").and_then(|(_, r)| r.split_once('/')).map(|(_, p)| p).unwrap_or("");
            let path = path.split(['?', '#']).next().unwrap_or("");
            let file = if path.is_empty() { file } else { path };
            let (status, mime, body) = match crate::scripts::serve(&format!("https://x/{file}")) {
                Some((m, b)) => (200, m, b),
                None => (404, "text/plain", b"not found".to_vec()),
            };
            let stream = SHCreateMemStream(Some(&body));
            if let Ok(resp) = env.CreateWebResourceResponse(
                stream.as_ref(),
                status,
                w!("OK"),
                &HSTRING::from(format!("Content-Type: {mime}\r\nCache-Control: no-store")),
            ) {
                let _ = args.SetResponse(&resp);
            }
            return;
        }
        // Built-in new-tab page, served from memory.
        if util::host_of(&uri) == newtab::HOST {
            let body = newtab::html(search).into_bytes();
            let stream = SHCreateMemStream(Some(&body));
            if let Ok(resp) = env.CreateWebResourceResponse(stream.as_ref(), 200, w!("OK"), w!("Content-Type: text/html; charset=utf-8")) {
                let _ = args.SetResponse(&resp);
            }
            return;
        }
        if !enabled {
            return;
        }
        // Never block the page the user navigated to.
        if ctx == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT && uri == doc_url {
            return;
        }
        let response = match adblock::check(&uri, &doc_url, ctx) {
            adblock::Verdict::Allow => return,
            adblock::Verdict::Block => env.CreateWebResourceResponse(None, 403, w!("Blocked"), w!("")),
            adblock::Verdict::Redirect(mime, body) => {
                let stream = SHCreateMemStream(Some(&body));
                env.CreateWebResourceResponse(
                    stream.as_ref(),
                    200,
                    w!("OK"),
                    &HSTRING::from(format!("Content-Type: {mime}\r\nAccess-Control-Allow-Origin: *")),
                )
            }
        };
        if let Ok(resp) = response {
            let _ = args.SetResponse(&resp);
            app::with(|s| {
                // Counted only (shown in the ≡ menu); no bar redraw per request.
                if let Some(t) = s.tab_mut(id) {
                    t.blocked += 1;
                }
            });
        }
    }
}
