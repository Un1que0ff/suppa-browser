// Window buttons (minimize / maximize / close) floating over the top-right
// corner of the page, used when no bar row is at the top (Settings -> Layout).
// A small layered child window drawn here pixel by pixel, with smooth edges,
// in the interface colours and corner style (round or squircle).
use std::cell::RefCell;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use crate::app::{self, *};
use crate::{ui, util};

const WM_MOUSELEAVE: u32 = 0x02A3;
/// Size of one button and the gap to the window edges, in CSS px.
const BTN_W: i32 = 40;
const BTN_H: i32 = 30;
const MARGIN: i32 = 8;

#[derive(Default)]
struct Btns {
    hwnd: HWND,
    hover: Option<usize>,
    pressed: Option<usize>,
}

thread_local! {
    static B: RefCell<Btns> = RefCell::new(Btns::default());
}

pub fn init(parent: HWND) {
    unsafe {
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: hinst.into(),
            lpszClassName: w!("SuppaWinButtons"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let h = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOACTIVATE,
            w!("SuppaWinButtons"),
            w!(""),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            Some(parent),
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap_or_default();
        B.with(|b| b.borrow_mut().hwnd = h);
    }
}

/// Places (or hides) the buttons and draws them.
pub fn layout() {
    let h = B.with(|b| b.borrow().hwnd);
    if h.is_invalid() {
        return;
    }
    let r = app::with(|s| (ui::floating_buttons(s) && !s.minimized, s.hwnd, ui::scaled(BTN_W, s), ui::scaled(BTN_H, s), ui::scaled(MARGIN, s)));
    let Some((show, main, bw, bh, m)) = r else { return };
    unsafe {
        if !show {
            let _ = ShowWindow(h, SW_HIDE);
            return;
        }
        let mut rc = RECT::default();
        let _ = GetClientRect(main, &mut rc);
        let w = bw * 3;
        // Above the page (new pages are created on top, so this runs again after them).
        let _ = SetWindowPos(h, Some(HWND_TOP), rc.right - w - m, m, w, bh, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    draw();
}

/// Redraws with the current colours (theme changed).
pub fn refresh() {
    draw();
}

fn draw() {
    let (h, hover, pressed) = B.with(|b| {
        let b = b.borrow();
        (b.hwnd, b.hover, b.pressed)
    });
    if h.is_invalid() || !unsafe { IsWindowVisible(h).as_bool() } {
        return;
    }
    let Some((cols, radius, squircle, line, scale, maxed, bw, bh)) = app::with(|s| {
        let st = &s.settings;
        let c = |n| ui::palette_color(st, n);
        let sq = st.corner_shape == "squircle";
        (
            [c("btn"), c("btn-hover"), c("fg"), c("border")],
            // Same as --r-btn in ui.css (squircles get a 1.6x bigger radius).
            ((if sq { (st.radius as f64 * 1.6).round() } else { st.radius as f64 }) - 1.0).max(0.0),
            sq,
            // Line width of the icons per icon style (same as --ic-win in ui.css).
            match st.icon_pack.as_str() {
                "bold" => 1.5,
                "filled" => 1.3,
                "duotone" => 1.2,
                _ => 1.0,
            },
            s.dpi as f64 / 96.0 * ui::zoom(s),
            unsafe { IsZoomed(s.hwnd).as_bool() },
            ui::scaled(BTN_W, s),
            ui::scaled(BTN_H, s),
        )
    }) else {
        return;
    };
    let (w, hgt) = (bw * 3, bh);
    let mut px = vec![[0f64; 4]; (w * hgt) as usize]; // premultiplied r, g, b, a
    let rad = (radius * scale).min(hgt as f64 / 2.0);
    let hover_col = |i: usize| if i == 2 { rgb(0xe8, 0x11, 0x23) } else { cols[1] };
    for y in 0..hgt {
        for x in 0..w {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            let outer = cover(shape_dist(fx, fy, w as f64, hgt as f64, rad, squircle));
            if outer <= 0.0 {
                continue;
            }
            let inner = cover(shape_dist(fx, fy, w as f64, hgt as f64, rad, squircle) + scale.max(1.0));
            let p = &mut px[(y * w + x) as usize];
            over(p, cols[3], outer); // outline
            over(p, cols[0], inner); // fill
            let i = (x / bw) as usize;
            if hover == Some(i) || pressed == Some(i) {
                over(p, hover_col(i), inner);
            }
        }
    }
    // Icons: 10 x 10 CSS px, centred in each button, 1 px lines.
    let icon = |i: usize| -> Vec<[f64; 4]> {
        match i {
            0 => vec![[0.0, 5.0, 10.0, 5.0]],
            1 if maxed => vec![
                [0.0, 2.5, 7.5, 2.5], [7.5, 2.5, 7.5, 10.0], [7.5, 10.0, 0.0, 10.0], [0.0, 10.0, 0.0, 2.5],
                [2.5, 2.5, 2.5, 0.0], [2.5, 0.0, 10.0, 0.0], [10.0, 0.0, 10.0, 7.5], [10.0, 7.5, 7.5, 7.5],
            ],
            1 => vec![[0.0, 0.0, 10.0, 0.0], [10.0, 0.0, 10.0, 10.0], [10.0, 10.0, 0.0, 10.0], [0.0, 10.0, 0.0, 0.0]],
            _ => vec![[0.0, 0.0, 10.0, 10.0], [10.0, 0.0, 0.0, 10.0]],
        }
    };
    let half = (scale * line * 0.5).max(0.5);
    for i in 0..3 {
        let fg = if i == 2 && (hover == Some(2) || pressed == Some(2)) { rgb(255, 255, 255) } else { cols[2] };
        let ox = i as f64 * bw as f64 + (bw as f64 - 10.0 * scale) / 2.0;
        let oy = (bh as f64 - 10.0 * scale) / 2.0;
        let segs: Vec<[f64; 4]> = icon(i)
            .into_iter()
            .map(|[a, b, c, d]| [ox + (a + 0.5) * scale, oy + (b + 0.5) * scale, ox + (c + 0.5) * scale, oy + (d + 0.5) * scale])
            .collect();
        let x0 = ox.floor() as i32 - 2;
        let y0 = oy.floor() as i32 - 2;
        let n = (11.0 * scale).ceil() as i32 + 4;
        for y in y0.max(0)..(y0 + n).min(hgt) {
            for x in x0.max(0)..(x0 + n).min(w) {
                let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                let d = segs.iter().map(|s| seg_dist(fx, fy, s)).fold(f64::MAX, f64::min) - half;
                let c = cover(d);
                if c > 0.0 {
                    over(&mut px[(y * w + x) as usize], fg, c);
                }
            }
        }
    }
    present(h, w, hgt, &px);
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// Pixel coverage from a signed distance (negative = inside).
fn cover(d: f64) -> f64 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Paints colour `c` with coverage `a` over a premultiplied pixel.
fn over(p: &mut [f64; 4], c: COLORREF, a: f64) {
    if a <= 0.0 {
        return;
    }
    let (r, g, b) = ((c.0 & 255) as f64 / 255.0, ((c.0 >> 8) & 255) as f64 / 255.0, ((c.0 >> 16) & 255) as f64 / 255.0);
    let k = 1.0 - a;
    p[0] = r * a + p[0] * k;
    p[1] = g * a + p[1] * k;
    p[2] = b * a + p[2] * k;
    p[3] = a + p[3] * k;
}

/// Signed distance to a w x h box with corner radius r (round, or squircle =
/// superellipse with exponent 4, like CSS corner-shape: squircle).
fn shape_dist(x: f64, y: f64, w: f64, h: f64, r: f64, squircle: bool) -> f64 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let qx = (x - hw).abs() - (hw - r);
    let qy = (y - hh).abs() - (hh - r);
    let (ax, ay) = (qx.max(0.0), qy.max(0.0));
    let len = if squircle { (ax.powi(4) + ay.powi(4)).powf(0.25) } else { (ax * ax + ay * ay).sqrt() };
    len + qx.max(qy).min(0.0) - r
}

fn seg_dist(x: f64, y: f64, s: &[f64; 4]) -> f64 {
    let (ax, ay, bx, by) = (s[0], s[1], s[2], s[3]);
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy).max(1e-9)).clamp(0.0, 1.0);
    ((x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2)).sqrt()
}

/// Hands the pixels to Windows (per-pixel alpha, so the corners are smooth).
fn present(h: HWND, w: i32, hgt: i32, px: &[[f64; 4]]) {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -hgt,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            return;
        };
        let out = std::slice::from_raw_parts_mut(bits as *mut u8, (w * hgt * 4) as usize);
        for (i, p) in px.iter().enumerate() {
            let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            out[i * 4] = q(p[2]);
            out[i * 4 + 1] = q(p[1]);
            out[i * 4 + 2] = q(p[0]);
            out[i * 4 + 3] = q(p[3]);
        }
        let old = SelectObject(mem, HGDIOBJ(dib.0));
        let size = SIZE { cx: w, cy: hgt };
        let src = POINT::default();
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let _ = UpdateLayeredWindow(h, None, None, Some(&size), Some(mem), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(dib.0));
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

fn button_at(lp: LPARAM) -> Option<usize> {
    let x = (lp.0 & 0xFFFF) as i16 as i32;
    let bw = app::with(|s| ui::scaled(BTN_W, s)).unwrap_or(40).max(1);
    (x >= 0).then(|| (x / bw) as usize).filter(|&i| i < 3)
}

unsafe extern "system" fn proc_(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                let i = button_at(lp);
                let changed = B.with(|b| {
                    let mut b = b.borrow_mut();
                    let c = b.hover != i;
                    b.hover = i;
                    c
                });
                if changed {
                    let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut t);
                    draw();
                }
                return LRESULT(0);
            }
            WM_MOUSELEAVE => {
                B.with(|b| b.borrow_mut().hover = None);
                draw();
                return LRESULT(0);
            }
            WM_LBUTTONDOWN => {
                SetCapture(h);
                B.with(|b| b.borrow_mut().pressed = button_at(lp));
                draw();
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let pressed = B.with(|b| b.borrow_mut().pressed.take());
                let mut rc = RECT::default();
                let _ = GetClientRect(h, &mut rc);
                let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
                let i = button_at(lp).filter(|_| y >= 0 && y < rc.bottom);
                if pressed.is_some() && pressed == i {
                    let name = ["minimize", "maximize", "close_window"][i.unwrap_or(0)];
                    if let Some(cmd) = ui::command_for(name) {
                        util::post(app::hwnd(), WM_APP_COMMAND, cmd as usize, 0);
                    }
                }
                B.with(|b| b.borrow_mut().hover = None);
                draw();
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(h, msg, wp, lp)
    }
}
