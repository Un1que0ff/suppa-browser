// Extension popups: clicking an extension's button next to the address bar
// opens its popup page (chrome-extension://<id>/<popup.html>) in this small
// window under the button, sized to the page like in Chrome (max 800 x 600).
// Closes on Esc, a click outside, switching apps, or window.close() in the page.
use std::cell::RefCell;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PWSTR, w};

use crate::{app, tabs, util};

const TIMER_OUTSIDE: usize = 8;
const MAX_W: i32 = 800;
const MAX_H: i32 = 600;

/// Measures the popup page and reports its size (runs in every page of this window).
const MEASURE_SCRIPT: &str = r#"(function () {
  if (location.protocol !== 'chrome-extension:') return;
  let busy = false;
  function measure() {
    const b = document.body;
    if (!b || busy) return;
    busy = true;
    let w = Math.ceil(b.getBoundingClientRect().width);
    if (w >= window.innerWidth - 1) {          // body fills the window: use its natural width
      const h = document.documentElement, prev = h.style.width;
      h.style.width = 'max-content';
      w = Math.ceil(h.getBoundingClientRect().width);
      h.style.width = prev;
    }
    const hh = Math.ceil(Math.max(document.documentElement.scrollHeight, b.scrollHeight));
    busy = false;
    window.chrome.webview.postMessage({ suppaExtSize: [w, hh] });
  }
  let t = 0;
  const later = () => { clearTimeout(t); t = setTimeout(measure, 30); };
  document.addEventListener('DOMContentLoaded', () => {
    measure();
    new ResizeObserver(later).observe(document.body);
    setTimeout(measure, 300);
  });
  addEventListener('load', later);
})();"#;

#[derive(Default)]
struct ExtPopup {
    hwnd: HWND,
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    creating: bool,
    /// URL to open once the WebView exists.
    pending: Option<String>,
    anchor: POINT,
    visible: bool,
    size: (i32, i32),
    fg_at_show: isize,
    buttons_down: bool,
}

thread_local! {
    static E: RefCell<ExtPopup> = RefCell::new(ExtPopup::default());
}

fn with<R>(f: impl FnOnce(&mut ExtPopup) -> R) -> Option<R> {
    E.with(|p| p.try_borrow_mut().ok().map(|mut g| f(&mut g)))
}

pub fn init(owner: HWND) {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("SuppaExtPopup");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        if let Ok(h) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class,
            w!("Extension"),
            WS_POPUP | WS_BORDER | WS_CLIPCHILDREN,
            -32000,
            -32000,
            MAX_W,
            MAX_H,
            Some(owner),
            None,
            Some(hinst.into()),
            None,
        ) {
            with(|p| p.hwnd = h);
        }
    }
}

/// Opens an extension popup page under the toolbar button (anchor = button's
/// bottom-right corner, screen px).
pub fn open(url: &str, anchor: POINT) {
    let Some((hwnd, wv, creating)) = with(|p| {
        p.anchor = anchor;
        p.size = (0, 0);
        (p.hwnd, p.webview.clone(), p.creating)
    }) else {
        return;
    };
    if hwnd.is_invalid() {
        return;
    }
    hide();
    unsafe {
        // Lay the page out at the maximum size while hidden, then shrink to fit.
        let _ = SetWindowPos(hwnd, None, -32000, -32000, MAX_W, MAX_H, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    match wv {
        Some(wv) => {
            if let Some(Some(c)) = with(|p| p.controller.clone()) {
                unsafe {
                    let _ = c.SetBounds(RECT { left: 0, top: 0, right: MAX_W, bottom: MAX_H });
                }
            }
            unsafe {
                let _ = wv.Navigate(&HSTRING::from(url));
            }
        }
        None => {
            with(|p| p.pending = Some(url.to_string()));
            if !creating {
                create(hwnd);
            }
        }
    }
}

fn create(hwnd: HWND) {
    let Some(Some(env)) = app::with(|s| s.env.clone()) else { return };
    with(|p| p.creating = true);
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |res, c| {
        with(|p| p.creating = false);
        if let (Ok(()), Some(c)) = (res, c) {
            setup(c);
        }
        Ok(())
    }));
    unsafe {
        let _ = env.CreateCoreWebView2Controller(hwnd, &handler);
    }
}

fn setup(c: ICoreWebView2Controller) {
    let Ok(wv) = (unsafe { c.CoreWebView2() }) else { return };
    let mut token = 0i64;
    unsafe {
        let _ = c.SetBounds(RECT { left: 0, top: 0, right: MAX_W, bottom: MAX_H });
        if let Ok(st) = wv.Settings() {
            let _ = st.SetIsStatusBarEnabled(false);
            let _ = st.SetIsZoomControlEnabled(false);
        }
        let _ = wv.AddScriptToExecuteOnDocumentCreated(
            &HSTRING::from(MEASURE_SCRIPT),
            None::<&ICoreWebView2AddScriptToExecuteOnDocumentCreatedCompletedHandler>,
        );
        // Size reports from the page.
        let _ = wv.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut p = PWSTR::null();
                let _ = args.WebMessageAsJson(&mut p);
                let json = util::take_pwstr(p);
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
                    if let Some(a) = v.get("suppaExtSize").and_then(|a| a.as_array()) {
                        let w = a.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as i32;
                        let h = a.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0) as i32;
                        place(w, h);
                    }
                }
                Ok(())
            })),
            &mut token,
        );
        // window.close() in the popup.
        let _ = wv.add_WindowCloseRequested(
            &WindowCloseRequestedEventHandler::create(Box::new(|_, _| {
                util::post(with(|p| p.hwnd).unwrap_or_default(), WM_CLOSE, 0, 0);
                Ok(())
            })),
            &mut token,
        );
        // Links from the popup open as normal tabs.
        let _ = wv.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut p = PWSTR::null();
                let _ = args.Uri(&mut p);
                let uri = util::take_pwstr(p);
                let _ = args.SetHandled(true);
                tabs::new_tab(Some(uri), true);
                util::post(with(|p| p.hwnd).unwrap_or_default(), WM_CLOSE, 0, 0);
                Ok(())
            })),
            &mut token,
        );
        // Esc closes it.
        let _ = c.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut vk = 0u32;
                let _ = args.VirtualKey(&mut vk);
                if vk == 0x1B {
                    let _ = args.SetHandled(true);
                    util::post(with(|p| p.hwnd).unwrap_or_default(), WM_CLOSE, 0, 0);
                }
                Ok(())
            })),
            &mut token,
        );
    }
    let pending = with(|p| {
        p.controller = Some(c);
        p.webview = Some(wv.clone());
        p.pending.take()
    })
    .flatten();
    if let Some(url) = pending {
        unsafe {
            let _ = wv.Navigate(&HSTRING::from(url));
        }
    }
}

/// Fits the window to the page (CSS px) and shows it under the button.
fn place(wc: i32, hc: i32) {
    let Some((hwnd, anchor, ctrl, was_visible, old)) = with(|p| (p.hwnd, p.anchor, p.controller.clone(), p.visible, p.size)) else {
        return;
    };
    if wc < 10 || hc < 10 || (was_visible && old == (wc, hc)) {
        return;
    }
    with(|p| p.size = (wc, hc));
    let dpi = app::with(|s| s.dpi).unwrap_or(96) as f64 / 96.0;
    let border = 2;
    let w = ((wc.clamp(25, MAX_W) as f64) * dpi).ceil() as i32 + border;
    let h = ((hc.clamp(25, MAX_H) as f64) * dpi).ceil() as i32 + border;
    unsafe {
        let mon = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let wa = mi.rcWork;
        let x = (anchor.x - w).clamp(wa.left, (wa.right - w).max(wa.left));
        let y = if anchor.y + h > wa.bottom { (wa.bottom - h).max(wa.top) } else { anchor.y };
        let _ = SetWindowPos(hwnd, Some(HWND_TOP), x, y, w, h, SWP_SHOWWINDOW);
        if let Some(c) = &ctrl {
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let _ = c.SetBounds(rc);
            let _ = c.SetIsVisible(true);
        }
        if !was_visible {
            with(|p| {
                p.visible = true;
                p.fg_at_show = GetForegroundWindow().0 as isize;
                p.buttons_down = buttons_down();
            });
            let _ = SetForegroundWindow(hwnd);
            if let Some(c) = &ctrl {
                let _ = c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
            SetTimer(Some(hwnd), TIMER_OUTSIDE, 40, None);
        }
    }
}

/// Hides the popup and unloads the extension page.
pub fn hide() {
    let Some((hwnd, wv, ctrl, was)) = with(|p| {
        let was = std::mem::replace(&mut p.visible, false);
        (p.hwnd, p.webview.clone(), p.controller.clone(), was)
    }) else {
        return;
    };
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_OUTSIDE);
        let _ = ShowWindow(hwnd, SW_HIDE);
        if let Some(c) = ctrl {
            let _ = c.SetIsVisible(false);
        }
        if let Some(wv) = wv {
            let _ = wv.Navigate(w!("about:blank"));
        }
        if was && GetForegroundWindow() == hwnd {
            let _ = SetForegroundWindow(app::hwnd());
        }
    }
}

fn buttons_down() -> bool {
    unsafe { [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| GetAsyncKeyState(k.0 as i32) as u16 & 0x8000 != 0) }
}

/// A click outside the popup or switching to another app closes it.
fn check_outside(hwnd: HWND) {
    unsafe {
        let down = buttons_down();
        let prev = with(|p| std::mem::replace(&mut p.buttons_down, down)).unwrap_or(false);
        let mut outside = false;
        if down && !prev {
            let (mut pt, mut rc) = (POINT::default(), RECT::default());
            let _ = GetCursorPos(&mut pt);
            let _ = GetWindowRect(hwnd, &mut rc);
            outside = pt.x < rc.left || pt.x >= rc.right || pt.y < rc.top || pt.y >= rc.bottom;
        }
        let fg = GetForegroundWindow();
        let at_show = with(|p| p.fg_at_show).unwrap_or(0);
        let other_app = !fg.is_invalid() && fg != hwnd && fg != app::hwnd() && fg.0 as isize != at_show
            && GetWindow(fg, GW_OWNER).map(|o| o != app::hwnd()).unwrap_or(true);
        if outside || other_app {
            hide();
        }
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER if wp.0 == TIMER_OUTSIDE => {
                check_outside(hwnd);
                LRESULT(0)
            }
            WM_CLOSE => {
                hide();
                LRESULT(0)
            }
            WM_SIZE => {
                if let Some(Some(c)) = with(|p| p.controller.clone()) {
                    let mut rc = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rc);
                    let _ = c.SetBounds(rc);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
