// Build script:
//  1. Embeds app.manifest (modern Windows controls + per-monitor DPI).
//  2. Draws assets\icon.svg into multi-size Windows icons and compiles them
//     into Suppa.exe (no separate icon file is needed at runtime):
//     icon 1 = white shape on a see-through background (dark Windows theme,
//     also the file's icon), icon 2 = black (light Windows theme). Suppa picks
//     the one for the Windows theme for its window and taskbar button.
//     Replace assets\icon.svg and rebuild (or press Update) to change the icon.
//     Supported SVG: <path d="..."> with M/L/H/V/Z commands (straight lines).
use std::path::{Path, PathBuf};

const SIZES: [u32; 7] = [16, 20, 24, 32, 48, 64, 256];
/// Badge behind the shape so a black icon stays visible on a dark taskbar.
/// Set BADGE to None for a transparent background.
const BADGE: Option<[u8; 4]> = None; // e.g. Some([255, 255, 255, 255]) (RGBA)
const SHAPE: [u8; 3] = [0, 0, 0]; // used when the SVG has no fill colour
const PADDING: f32 = 0.12; // fraction of the icon left around the shape
/// Invert the colours (shape and badge): black-on-white becomes white-on-black.
const INVERT: bool = true;
/// Badge gradient (top-left -> bottom-right). None = use BADGE (inverted if INVERT).
const GRADIENT: Option<([u8; 3], [u8; 3])> = Some(([0x5a, 0x5a, 0x5a], [0x1c, 0x1c, 0x1c]));

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.svg");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let manifest = root.join("app.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());

    let svg = root.join("assets").join("icon.svg");
    if let Ok(text) = std::fs::read_to_string(&svg) {
        let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let res = out.join("icon.res");
        match make_res(&text, &res) {
            Ok(()) => {
                println!("cargo:rustc-link-arg-bins={}", res.display());
                // The installer (installer\) uses the same icon.
                let _ = std::fs::copy(&res, root.join("target").join("suppa-icon.res"));
            }
            Err(e) => println!("cargo:warning=icon not embedded: {e}"),
        }
    }
}

// ---- SVG -> polygons ---------------------------------------------------------------

struct Shape {
    polys: Vec<Vec<(f32, f32)>>,
    view: (f32, f32, f32, f32),
    fill: [u8; 3],
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(&tag[i..j])
}

fn parse_svg(text: &str) -> Result<Shape, String> {
    let svg_tag = text.find("<svg").map(|i| &text[i..i + text[i..].find('>').unwrap_or(0)]).ok_or("no <svg>")?;
    let vb: Vec<f32> = attr(svg_tag, "viewBox")
        .ok_or("no viewBox")?
        .split([' ', ','])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if vb.len() != 4 {
        return Err("bad viewBox".into());
    }
    // First fill colour found in a style block or attribute.
    let mut fill = SHAPE;
    if let Some(i) = text.find("fill:#").map(|i| i + 6).or_else(|| text.find("fill=\"#").map(|i| i + 7)) {
        if let Ok(v) = u32::from_str_radix(text.get(i..i + 6).unwrap_or(""), 16) {
            fill = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        }
    }
    let mut polys = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("<path") {
        let end = rest[i..].find('>').map(|e| i + e).ok_or("bad <path>")?;
        let tag = &rest[i..end];
        if let Some(d) = attr(tag, "d") {
            parse_path(d, &mut polys)?;
        }
        rest = &rest[end..];
    }
    if polys.is_empty() {
        return Err("no paths".into());
    }
    Ok(Shape { polys, view: (vb[0], vb[1], vb[2], vb[3]), fill })
}

fn parse_path(d: &str, polys: &mut Vec<Vec<(f32, f32)>>) -> Result<(), String> {
    // Tokenise into commands and numbers.
    let mut toks: Vec<String> = Vec::new();
    let mut num = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            if !num.is_empty() {
                toks.push(std::mem::take(&mut num));
            }
            toks.push(c.to_string());
        } else if c == ' ' || c == ',' || c == '\n' || c == '\t' {
            if !num.is_empty() {
                toks.push(std::mem::take(&mut num));
            }
        } else if c == '-' && !num.is_empty() && !num.ends_with(['e', 'E']) {
            toks.push(std::mem::take(&mut num));
            num.push(c);
        } else {
            num.push(c);
        }
    }
    if !num.is_empty() {
        toks.push(num);
    }
    let mut cur: Vec<(f32, f32)> = Vec::new();
    let (mut x, mut y) = (0f32, 0f32);
    let mut cmd = 'M';
    let mut i = 0;
    let n = |i: usize| -> Result<f32, String> { toks.get(i).and_then(|t| t.parse().ok()).ok_or("bad number".to_string()) };
    while i < toks.len() {
        let t = &toks[i];
        if t.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
            cmd = t.chars().next().unwrap();
            i += 1;
            if cmd == 'Z' || cmd == 'z' {
                if cur.len() > 2 {
                    polys.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
            continue;
        }
        match cmd {
            'M' | 'L' | 'm' | 'l' => {
                let (a, b) = (n(i)?, n(i + 1)?);
                i += 2;
                if cmd.is_ascii_lowercase() {
                    x += a;
                    y += b;
                } else {
                    x = a;
                    y = b;
                }
                if cmd == 'M' || cmd == 'm' {
                    if cur.len() > 2 {
                        polys.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cmd = if cmd == 'M' { 'L' } else { 'l' };
                }
                cur.push((x, y));
            }
            'H' | 'h' => {
                let a = n(i)?;
                i += 1;
                x = if cmd == 'h' { x + a } else { a };
                cur.push((x, y));
            }
            'V' | 'v' => {
                let a = n(i)?;
                i += 1;
                y = if cmd == 'v' { y + a } else { a };
                cur.push((x, y));
            }
            other => return Err(format!("unsupported path command '{other}' (only straight lines)")),
        }
    }
    if cur.len() > 2 {
        polys.push(cur);
    }
    Ok(())
}

// ---- Rasterising ---------------------------------------------------------------------

/// Coverage (0..1) of polygons at `size` px, 4x4 supersampling, non-zero fill.
fn coverage(polys: &[Vec<(f32, f32)>], size: u32) -> Vec<f32> {
    const SS: u32 = 4;
    let s = size as usize;
    let mut cov = vec![0f32; s * s];
    let w = 1.0 / (SS * SS) as f32;
    for row in 0..size * SS {
        let sy = (row as f32 + 0.5) / SS as f32;
        // Crossings with winding direction.
        let mut xs: Vec<(f32, i32)> = Vec::new();
        for p in polys {
            for k in 0..p.len() {
                let (x0, y0) = p[k];
                let (x1, y1) = p[(k + 1) % p.len()];
                if (y0 <= sy) != (y1 <= sy) {
                    let t = (sy - y0) / (y1 - y0);
                    xs.push((x0 + t * (x1 - x0), if y1 > y0 { 1 } else { -1 }));
                }
            }
        }
        xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let py = (row / SS) as usize;
        let mut wind = 0;
        for k in 0..xs.len() {
            let before = wind;
            wind += xs[k].1;
            if before == 0 && wind != 0 {
                // span starts at xs[k].0, ends where wind returns to 0
                let start = xs[k].0;
                let mut j = k + 1;
                let mut w2 = wind;
                while j < xs.len() {
                    w2 += xs[j].1;
                    if w2 == 0 {
                        break;
                    }
                    j += 1;
                }
                let end = xs.get(j).map(|e| e.0).unwrap_or(start);
                for sub in 0..size * SS {
                    let sx = (sub as f32 + 0.5) / SS as f32;
                    if sx >= start && sx < end {
                        cov[py * s + (sub / SS) as usize] += w;
                    }
                }
            }
        }
    }
    cov
}

fn rounded_rect_cov(size: u32) -> Vec<f32> {
    let s = size as f32;
    let r = s * 0.22;
    let mut out = vec![0f32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut c = 0.0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = x as f32 + (sx as f32 + 0.5) / 4.0;
                    let py = y as f32 + (sy as f32 + 0.5) / 4.0;
                    let dx = (r - px).max(px - (s - r)).max(0.0);
                    let dy = (r - py).max(py - (s - r)).max(0.0);
                    if dx * dx + dy * dy <= r * r {
                        c += 1.0 / 16.0;
                    }
                }
            }
            out[(y * size + x) as usize] = c;
        }
    }
    out
}

/// `fill`: colour of the shape (None = the SVG's own, inverted if INVERT).
fn render(shape: &Shape, size: u32, fill: Option<[u8; 3]>) -> Vec<u8> {
    let inv = |c: [u8; 3]| if INVERT { [255 - c[0], 255 - c[1], 255 - c[2]] } else { c };
    let fill = fill.unwrap_or_else(|| inv(shape.fill));
    let badge_color = BADGE.map(|b| {
        let c = inv([b[0], b[1], b[2]]);
        [c[0], c[1], c[2], b[3]]
    });
    let (vx, vy, vw, vh) = shape.view;
    let pad = if BADGE.is_some() { PADDING } else { 0.04 };
    let inner = size as f32 * (1.0 - 2.0 * pad);
    let scale = inner / vw.max(vh);
    let ox = (size as f32 - vw * scale) / 2.0;
    let oy = (size as f32 - vh * scale) / 2.0;
    let polys: Vec<Vec<(f32, f32)>> = shape
        .polys
        .iter()
        .map(|p| p.iter().map(|&(x, y)| ((x - vx) * scale + ox, (y - vy) * scale + oy)).collect())
        .collect();
    let cov = coverage(&polys, size);
    let badge = BADGE.map(|_| rounded_rect_cov(size));
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for i in 0..(size * size) as usize {
        let a = cov[i].min(1.0);
        let (mut r, mut g, mut b, mut al) = (0f32, 0f32, 0f32, 0f32);
        if let (Some(bc), Some(bcov)) = (badge_color, &badge) {
            al = bcov[i] * bc[3] as f32 / 255.0;
            r = bc[0] as f32;
            g = bc[1] as f32;
            b = bc[2] as f32;
            if let Some((c0, c1)) = GRADIENT {
                let (px, py) = ((i as u32 % size) as f32, (i as u32 / size) as f32);
                let t = ((px + py) / (2.0 * size as f32 - 2.0).max(1.0)).clamp(0.0, 1.0);
                let lerp = |a: u8, b: u8| a as f32 + (b as f32 - a as f32) * t;
                r = lerp(c0[0], c1[0]);
                g = lerp(c0[1], c1[1]);
                b = lerp(c0[2], c1[2]);
            }
        }
        // shape over badge
        let out_a = a + al * (1.0 - a);
        if out_a > 0.0 {
            let mix = |s: u8, d: f32| (s as f32 * a + d * al * (1.0 - a)) / out_a;
            r = mix(fill[0], r);
            g = mix(fill[1], g);
            b = mix(fill[2], b);
        }
        rgba[i * 4] = r.round() as u8;
        rgba[i * 4 + 1] = g.round() as u8;
        rgba[i * 4 + 2] = b.round() as u8;
        rgba[i * 4 + 3] = (out_a * 255.0).round() as u8;
    }
    rgba
}

fn png(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, size, size);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Best);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(rgba).unwrap();
    }
    out
}

// ---- .res file (compiled resources, linked straight into the exe) ----------------------

fn res_entry(out: &mut Vec<u8>, typ: u16, name: u16, flags: u16, data: &[u8]) {
    let push32 = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    let push16 = |o: &mut Vec<u8>, v: u16| o.extend_from_slice(&v.to_le_bytes());
    push32(out, data.len() as u32); // DataSize
    push32(out, 32); // HeaderSize
    push16(out, 0xFFFF);
    push16(out, typ);
    push16(out, 0xFFFF);
    push16(out, name);
    push32(out, 0); // DataVersion
    push16(out, flags); // MemoryFlags
    push16(out, 0x0409); // LanguageId (en-US)
    push32(out, 0); // Version
    push32(out, 0); // Characteristics
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn make_res(svg: &str, res: &Path) -> Result<(), String> {
    let shape = parse_svg(svg)?;
    let mut out = Vec::new();
    res_entry(&mut out, 0, 0, 0, &[]); // mandatory empty first entry
    // Group 1: white shape (dark theme), group 2: black shape (light theme).
    for (gid, colour, first) in [(1u16, [255u8, 255, 255], 1u16), (2, [0, 0, 0], 101)] {
        let mut group = vec![0u8, 0, 1, 0];
        group.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
        for (k, &size) in SIZES.iter().enumerate() {
            let img = png(&render(&shape, size, Some(colour)), size);
            let id = first + k as u16;
            res_entry(&mut out, 3, id, 0x1010, &img); // RT_ICON
            let dim = if size >= 256 { 0 } else { size as u8 };
            group.extend_from_slice(&[dim, dim, 0, 0]);
            group.extend_from_slice(&1u16.to_le_bytes()); // planes
            group.extend_from_slice(&32u16.to_le_bytes()); // bit count
            group.extend_from_slice(&(img.len() as u32).to_le_bytes());
            group.extend_from_slice(&id.to_le_bytes());
        }
        res_entry(&mut out, 14, gid, 0x1030, &group); // RT_GROUP_ICON
    }
    std::fs::write(res, out).map_err(|e| e.to_string())
}
