// The setup's window, drawn in Suppa's own style: same colours, accent and corner
// roundness as the browser (taken from your Suppa settings when they exist, so a
// custom accent or the light theme carry over). One window is reused for every
// step: questions (dialog) and work in progress (busy, with a moving bar).
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, TRACKMOUSEEVENT, TME_LEAVE, TrackMouseEvent, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, w};

const WM_APP_DONE: u32 = WM_APP + 1;
const WM_MOUSELEAVE: u32 = 0x02A3;

// ---- theme -----------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Theme {
    panel: u32,
    btn: u32,
    btn_hover: u32,
    fg: u32,
    muted: u32,
    border: u32,
    trough: u32,
    accent: u32,
    on_accent: u32,
    radius: i32,
}

/// "#rrggbb" -> COLORREF (0x00bbggrr)
fn hex(s: &str) -> Option<u32> {
    let s = s.trim().strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(((v & 0xff) << 16) | (v & 0xff00) | ((v >> 16) & 0xff))
}

fn rgb(c: u32) -> (f32, f32, f32) {
    ((c & 0xff) as f32, ((c >> 8) & 0xff) as f32, ((c >> 16) & 0xff) as f32)
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    let m = |x: f32, y: f32| ((x + (y - x) * t).round() as u32).min(255);
    m(ar, br) | (m(ag, bg) << 8) | (m(ab, bb) << 16)
}

fn windows_dword(path: PCWSTR, name: PCWSTR) -> Option<u32> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let mut v = 0u32;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, path, name, RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut size))
            .ok()
            .ok()?;
    }
    Some(v)
}

fn load_theme() -> Theme {
    let ini = std::env::var_os("LOCALAPPDATA")
        .map(|d| std::path::PathBuf::from(d).join("SuppaBrowser").join("settings.ini"))
        .and_then(|p| std::fs::read_to_string(p).ok());
    let get = |key: &str| -> Option<String> {
        ini.as_deref()?.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=').map(|v| v.trim().to_string()))
    };
    let dark = match get("theme") {
        Some(t) => t != "light",
        // Not installed yet: follow Windows' app theme.
        None => windows_dword(w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), w!("AppsUseLightTheme")) != Some(1),
    };
    let mut t = if dark {
        Theme {
            panel: hex("#1f1f1f").unwrap(),
            btn: hex("#2e2e2e").unwrap(),
            btn_hover: hex("#3b3b3b").unwrap(),
            fg: hex("#ececec").unwrap(),
            muted: hex("#8e8e8e").unwrap(),
            border: hex("#383838").unwrap(),
            trough: hex("#2b2b2b").unwrap(),
            accent: hex("#b07cff").unwrap(),
            on_accent: hex("#1c1c1c").unwrap(),
            radius: 10,
        }
    } else {
        Theme {
            panel: hex("#ffffff").unwrap(),
            btn: hex("#e5e5e5").unwrap(),
            btn_hover: hex("#d8d8d8").unwrap(),
            fg: hex("#1c1c1c").unwrap(),
            muted: hex("#6b6b6b").unwrap(),
            border: hex("#d0d0d0").unwrap(),
            trough: hex("#dcdcdc").unwrap(),
            accent: hex("#b07cff").unwrap(),
            on_accent: hex("#1c1c1c").unwrap(),
            radius: 10,
        }
    };
    match get("accent").as_deref() {
        Some("windows") => {
            if let Some(c) = windows_dword(w!(r"Software\Microsoft\Windows\DWM"), w!("AccentColor")) {
                t.accent = c & 0x00ff_ffff; // stored as 0xAABBGGRR
            }
        }
        Some(c) => {
            if let Some(c) = hex(c) {
                t.accent = c;
            }
        }
        None => {}
    }
    if let Some(r) = get("radius").and_then(|r| r.parse::<i32>().ok()) {
        t.radius = r.clamp(0, 20);
    }
    let (r, g, b) = rgb(t.accent);
    let auto_dark_text = 0.299 * r + 0.587 * g + 0.114 * b > 150.0;
    t.on_accent = match get("on_accent").as_deref() {
        Some("white") => hex("#ffffff").unwrap(),
        Some("black") => hex("#1c1c1c").unwrap(),
        _ if auto_dark_text => hex("#1c1c1c").unwrap(),
        _ => hex("#ffffff").unwrap(),
    };
    t
}

// ---- window state ----------------------------------------------------------------------

struct State {
    hwnd: HWND,
    theme: Theme,
    scale: f32,
    title: String,
    text: String,
    buttons: Vec<String>,
    primary: usize,
    busy: bool,
    hover: Option<usize>,
    pressed: Option<usize>,
    choice: Option<Option<usize>>,
    btn_rects: Vec<RECT>,
    started: Instant,
    font_title: HFONT,
    font_text: HFONT,
    font_btn: HFONT,
    icon: HICON,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn px(s: f32, v: i32) -> i32 {
    (v as f32 * s).round() as i32
}

fn font(height: i32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

const WIDTH: i32 = 460;
const PAD: i32 = 26;
const ICON: i32 = 40;
const BTN_H: i32 = 34;

/// Lays everything out for the current content; returns the window size.
fn layout(st: &mut State) -> (i32, i32) {
    let s = st.scale;
    let w = px(s, WIDTH);
    unsafe {
        let dc = GetDC(None);
        // body text height
        let old = SelectObject(dc, st.font_text.into());
        let mut r = RECT { left: 0, top: 0, right: w - 2 * px(s, PAD), bottom: 0 };
        let mut t = wide(&st.text);
        DrawTextW(dc, &mut t, &mut r, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
        let text_h = r.bottom - r.top;
        let body_top = px(s, PAD + ICON + 18);
        let mut y = body_top + text_h + px(s, 24);
        // buttons, right aligned, primary on the right
        SelectObject(dc, st.font_btn.into());
        st.btn_rects.clear();
        let mut x = w - px(s, PAD);
        let mut rects = vec![RECT::default(); st.buttons.len()];
        let mut order: Vec<usize> = (0..st.buttons.len()).filter(|&i| i != st.primary).collect();
        if st.primary < st.buttons.len() {
            order.insert(0, st.primary);
        }
        for &i in &order {
            let mut m = RECT::default();
            let mut b = wide(&st.buttons[i]);
            DrawTextW(dc, &mut b, &mut m, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
            let bw = (m.right - m.left + px(s, 36)).max(px(s, 96));
            rects[i] = RECT { left: x - bw, top: y, right: x, bottom: y + px(s, BTN_H) };
            x -= bw + px(s, 8);
        }
        st.btn_rects = rects;
        if st.busy {
            y = body_top + text_h + px(s, 20) + px(s, 6); // progress bar row
        } else {
            y += px(s, BTN_H);
        }
        SelectObject(dc, old);
        ReleaseDC(None, dc);
        (w, y + px(s, 22))
    }
}

fn apply_shape(st: &State, w: i32, h: i32) {
    unsafe {
        let d = px(st.scale, 2 * (st.theme.radius + 4));
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, d, d);
        SetWindowRgn(st.hwnd, Some(rgn), true);
    }
}

fn fill_round(dc: HDC, r: RECT, d: i32, fill: u32, outline: Option<u32>) {
    unsafe {
        let brush = CreateSolidBrush(COLORREF(fill));
        let pen = CreatePen(PS_SOLID, 1, COLORREF(outline.unwrap_or(fill)));
        let ob = SelectObject(dc, brush.into());
        let op = SelectObject(dc, pen.into());
        let _ = RoundRect(dc, r.left, r.top, r.right, r.bottom, d, d);
        SelectObject(dc, ob);
        SelectObject(dc, op);
        let _ = DeleteObject(brush.into());
        let _ = DeleteObject(pen.into());
    }
}

fn paint(st: &State, dc: HDC, w: i32, h: i32) {
    let t = st.theme;
    let s = st.scale;
    unsafe {
        // background + 1px border following the rounded window shape
        let bg = CreateSolidBrush(COLORREF(t.panel));
        FillRect(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, bg);
        let _ = DeleteObject(bg.into());
        let d = px(s, 2 * (t.radius + 4));
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, d, d);
        let border = CreateSolidBrush(COLORREF(t.border));
        let _ = FrameRgn(dc, rgn, border, 1, 1);
        let _ = DeleteObject(border.into());
        let _ = DeleteObject(rgn.into());

        SetBkMode(dc, TRANSPARENT);
        // icon + title
        let pad = px(s, PAD);
        let isz = px(s, ICON);
        if !st.icon.is_invalid() {
            let _ = DrawIconEx(dc, pad, pad, st.icon, isz, isz, 0, None, DI_NORMAL);
        }
        let old = SelectObject(dc, st.font_title.into());
        SetTextColor(dc, COLORREF(t.fg));
        let mut r = RECT { left: pad + isz + px(s, 14), top: pad, right: w - pad, bottom: pad + isz };
        let mut title = wide(&st.title);
        DrawTextW(dc, &mut title, &mut r, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);

        // body
        SelectObject(dc, st.font_text.into());
        SetTextColor(dc, COLORREF(mix(t.fg, t.muted, 0.25)));
        let top = px(s, PAD + ICON + 18);
        let mut r = RECT { left: pad, top, right: w - pad, bottom: h };
        let mut text = wide(&st.text);
        DrawTextW(dc, &mut text, &mut r, DT_WORDBREAK | DT_NOPREFIX);

        let rb = px(s, 2 * (t.radius - 1).max(0));
        if st.busy {
            // indeterminate progress bar
            let y = h - px(s, 22) - px(s, 6);
            let bar = RECT { left: pad, top: y, right: w - pad, bottom: y + px(s, 6) };
            fill_round(dc, bar, px(s, 6), t.trough, None);
            let span = (bar.right - bar.left) as f32;
            let seg = span * 0.3;
            let phase = (st.started.elapsed().as_secs_f32() / 1.4).fract();
            let x0 = bar.left as f32 - seg + phase * (span + seg);
            let l = (x0.max(bar.left as f32)) as i32;
            let rr = ((x0 + seg).min(bar.right as f32)) as i32;
            if rr - l > 2 {
                fill_round(dc, RECT { left: l, top: bar.top, right: rr, bottom: bar.bottom }, px(s, 6), t.accent, None);
            }
        } else {
            SelectObject(dc, st.font_btn.into());
            for (i, b) in st.buttons.iter().enumerate() {
                let r = st.btn_rects[i];
                let hot = st.hover == Some(i);
                let down = hot && st.pressed == Some(i);
                let (fill, fg) = if i == st.primary {
                    let base = t.accent;
                    (if down { mix(base, 0, 0.12) } else if hot { mix(base, 0xffffff, 0.14) } else { base }, t.on_accent)
                } else {
                    (if down { mix(t.btn_hover, 0, 0.08) } else if hot { t.btn_hover } else { t.btn }, t.fg)
                };
                fill_round(dc, r, rb, fill, None);
                SetTextColor(dc, COLORREF(fg));
                let mut rr = r;
                let mut txt = wide(b);
                DrawTextW(dc, &mut txt, &mut rr, DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_NOPREFIX);
            }
        }
        SelectObject(dc, old);
    }
}

fn button_at(st: &State, x: i32, y: i32) -> Option<usize> {
    if st.busy {
        return None;
    }
    st.btn_rects.iter().position(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let x = (lp.0 & 0xffff) as i16 as i32;
    let y = ((lp.0 >> 16) & 0xffff) as i16 as i32;
    unsafe {
        match msg {
            WM_NCHITTEST => {
                // drag the window anywhere except on the buttons
                let mut p = POINT { x, y };
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
                let on_btn = with(|st| button_at(st, p.x, p.y).is_some()).unwrap_or(false);
                return LRESULT(if on_btn { HTCLIENT } else { HTCAPTION } as isize);
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
                let changed = with(|st| {
                    let h = button_at(st, x, y);
                    let c = h != st.hover;
                    st.hover = h;
                    c
                });
                if changed == Some(true) {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return LRESULT(0);
            }
            WM_MOUSELEAVE => {
                with(|st| st.hover = None);
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_LBUTTONDOWN => {
                with(|st| st.pressed = button_at(st, x, y));
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                with(|st| {
                    let b = button_at(st, x, y);
                    if b.is_some() && b == st.pressed {
                        st.choice = Some(b);
                    }
                    st.pressed = None;
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_SETCURSOR => {
                if let Ok(c) = LoadCursorW(None, IDC_ARROW) {
                    SetCursor(Some(c));
                }
                return LRESULT(1);
            }
            WM_KEYDOWN => {
                let k = wp.0 as u16;
                with(|st| {
                    if st.busy {
                        return;
                    }
                    if k == VK_RETURN.0 {
                        st.choice = Some(Some(st.primary));
                    } else if k == VK_ESCAPE.0 {
                        st.choice = Some(None);
                    }
                });
                return LRESULT(0);
            }
            WM_CLOSE => {
                with(|st| {
                    if !st.busy {
                        st.choice = Some(None);
                    }
                });
                return LRESULT(0);
            }
            WM_TIMER => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                return LRESULT(0);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
                let (w, h) = (rc.right, rc.bottom);
                // double buffered: no flicker on hover / animation
                let mem = CreateCompatibleDC(Some(dc));
                let bmp = CreateCompatibleBitmap(dc, w, h);
                let old = SelectObject(mem, bmp.into());
                with(|st| paint(st, mem, w, h));
                let _ = BitBlt(dc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                let _ = DeleteDC(mem);
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

fn ensure_window(title: &str) -> HWND {
    if let Some(h) = with(|st| st.hwnd) {
        return h;
    }
    unsafe {
        let inst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("SuppaSetupWindow");
        let icon = LoadImageW(Some(inst.into()), PCWSTR(1 as _), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE).map(|h| HICON(h.0)).unwrap_or_default();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            lpszClassName: class,
            hIcon: icon,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let scale = GetDpiForSystem() as f32 / 96.0;
        let big = LoadImageW(Some(inst.into()), PCWSTR(1 as _), IMAGE_ICON, px(scale, ICON), px(scale, ICON), LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_default();
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class,
            &HSTRING::from(title),
            WS_POPUP | WS_SYSMENU | WS_MINIMIZEBOX,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(inst.into()),
            None,
        )
        .unwrap_or_default();
        let st = State {
            hwnd,
            theme: load_theme(),
            scale,
            title: String::new(),
            text: String::new(),
            buttons: Vec::new(),
            primary: 0,
            busy: false,
            hover: None,
            pressed: None,
            choice: None,
            btn_rects: Vec::new(),
            started: Instant::now(),
            font_title: font(px(scale, 18), 600),
            font_text: font(px(scale, 14), 400),
            font_btn: font(px(scale, 14), 500),
            icon: big,
        };
        STATE.with(|s| *s.borrow_mut() = Some(st));
        hwnd
    }
}

fn set_content(window_title: &str, title: &str, text: &str, buttons: &[&str], primary: usize, busy: bool) -> HWND {
    let hwnd = ensure_window(window_title);
    let first_show = unsafe { !IsWindowVisible(hwnd).as_bool() };
    let size = with(|st| {
        st.title = title.to_string();
        st.text = text.to_string();
        st.buttons = buttons.iter().map(|s| s.to_string()).collect();
        st.primary = primary;
        st.busy = busy;
        st.hover = None;
        st.pressed = None;
        st.choice = None;
        st.started = Instant::now();
        let (w, h) = layout(st);
        apply_shape(st, w, h);
        (w, h)
    });
    let Some((w, h)) = size else { return hwnd };
    unsafe {
        if first_show {
            // centre on the screen's work area
            let mut wa = RECT::default();
            let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
            let x = wa.left + (wa.right - wa.left - w) / 2;
            let y = wa.top + (wa.bottom - wa.top - h) / 2;
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        } else {
            // keep it centred where it is
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            let cy = (r.top + r.bottom) / 2;
            let _ = SetWindowPos(hwnd, None, r.left, cy - h / 2, w, h, SWP_NOZORDER);
        }
        let _ = SetFocus(Some(hwnd));
        if busy {
            SetTimer(Some(hwnd), 1, 16, None);
        } else {
            let _ = KillTimer(Some(hwnd), 1);
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    hwnd
}

fn pump_once() -> bool {
    unsafe {
        let mut m = MSG::default();
        if !GetMessageW(&mut m, None, 0, 0).as_bool() {
            return false;
        }
        let _ = TranslateMessage(&m);
        DispatchMessageW(&m);
        true
    }
}

/// Shows a question. `buttons[primary]` is the accent button (Enter); Esc or
/// closing gives None.
pub fn dialog(window_title: &str, title: &str, text: &str, buttons: &[&str], primary: usize) -> Option<usize> {
    set_content(window_title, title, text, buttons, primary, false);
    loop {
        if let Some(Some(c)) = with(|st| st.choice) {
            return c;
        }
        if !pump_once() {
            return None;
        }
    }
}

/// Shows `text` with a moving bar while `work` runs on another thread.
pub fn busy<T: Send>(window_title: &str, title: &str, text: &str, work: impl FnOnce() -> T + Send) -> T {
    let hwnd = set_content(window_title, title, text, &[], 0, true);
    let done = AtomicBool::new(false);
    let h = hwnd.0 as usize;
    let out = std::thread::scope(|s| {
        let job = s.spawn(|| {
            let r = work();
            done.store(true, Ordering::SeqCst);
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_APP_DONE, WPARAM(0), LPARAM(0));
            }
            r
        });
        while !done.load(Ordering::SeqCst) {
            if !pump_once() {
                break;
            }
        }
        job.join().unwrap()
    });
    unsafe {
        let _ = KillTimer(Some(hwnd), 1);
    }
    out
}

/// Saves a picture of a dialog as a .bmp without showing it (for checking the look:
/// `SuppaSetup.exe --preview <file.bmp>`).
pub fn preview(path: &std::path::Path, title: &str, text: &str, buttons: &[&str], primary: usize, busy: bool) {
    let hwnd = ensure_window("preview");
    let Some((w, h)) = with(|st| {
        st.title = title.to_string();
        st.text = text.to_string();
        st.buttons = buttons.iter().map(|s| s.to_string()).collect();
        st.primary = primary;
        st.busy = busy;
        st.hover = if busy { None } else { Some(1).filter(|&i| i < buttons.len()) };
        layout(st)
    }) else {
        return;
    };
    let _ = hwnd;
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        // outside the rounded corners: magenta, so the shape is visible
        let key = CreateSolidBrush(COLORREF(0x00ff00ff));
        FillRect(mem, &RECT { left: 0, top: 0, right: w, bottom: h }, key);
        let _ = DeleteObject(key.into());
        let d = with(|st| px(st.scale, 2 * (st.theme.radius + 4))).unwrap_or(0);
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, d, d);
        SelectClipRgn(mem, Some(rgn));
        with(|st| paint(st, mem, w, h));
        SelectClipRgn(mem, None);
        let _ = DeleteObject(rgn.into());
        // 32-bit top-down BMP
        let mut bi = BITMAPINFO::default();
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut px_data = vec![0u8; (w * h * 4) as usize];
        SelectObject(mem, old);
        GetDIBits(mem, bmp, 0, h as u32, Some(px_data.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        let mut file = Vec::new();
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&(54 + px_data.len() as u32).to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&54u32.to_le_bytes());
        let hdr = std::slice::from_raw_parts(&bi.bmiHeader as *const _ as *const u8, 40);
        file.extend_from_slice(hdr);
        file.extend_from_slice(&px_data);
        let _ = std::fs::write(path, file);
    }
}

/// Hides the window (e.g. before starting Suppa).
pub fn hide() {
    if let Some(h) = with(|st| st.hwnd) {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}
