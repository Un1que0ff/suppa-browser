// Media player button and per-tab sound.
//
//  * scripts\media.js runs in every page: it tells Suppa what the page plays
//    (title, artist, album cover from the page's media session - YouTube,
//    Spotify, Tidal, SoundCloud... set it), and gives each tab its own volume
//    (media elements and Web Audio), independent of the Windows volume.
//  * The player button next to the download button (≡ -> Look and toolbar ->
//    Show player button) opens a popup with a card per tab that plays: cover or
//    a small video preview, play / pause, previous / next, mute and a volume
//    slider for that tab only, and a click on the title goes to the tab.
//  * Tab menu -> Volume / Mute tab do the same from the tab row.
use std::cell::Cell;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::ExecuteScriptCompletedHandler;
use windows::core::{BOOL, HSTRING, Interface};

use crate::app::{self, *};
use crate::popup::{self, Item};
use crate::ui;

thread_local! {
    static OPEN: Cell<bool> = const { Cell::new(false) };
    /// Video previews (tab id -> small JPEG data URL), refreshed while the popup is open.
    static FRAMES: std::cell::RefCell<Vec<(u32, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn exec(wv: &ICoreWebView2, js: &str) {
    unsafe {
        // A frozen background tab is woken first, or the command would wait.
        if let Ok(w3) = wv.cast::<ICoreWebView2_3>() {
            let mut sus = BOOL(0);
            if w3.IsSuspended(&mut sus).is_ok() && sus.as_bool() {
                let _ = w3.Resume();
            }
        }
        let _ = wv.ExecuteScript(&HSTRING::from(js), None::<&ICoreWebView2ExecuteScriptCompletedHandler>);
    }
}

/// Tabs that have something to show in the player.
fn has_media(t: &Tab) -> bool {
    t.webview.is_some()
        && (t.audible || t.media.as_ref().is_some_and(|m| m["title"].as_str().is_some_and(|x| !x.is_empty()) || m["has"].as_bool().unwrap_or(false)))
}

/// `suppa-media:` message from a page (scripts\media.js).
pub fn on_page(id: u32, json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
    app::with(|s| {
        if let Some(t) = s.tab_mut(id) {
            t.media = if v["has"].as_bool().unwrap_or(false) { Some(v) } else { None };
        }
    });
    changed();
}

/// The engine says a tab started / stopped playing sound or was muted.
pub fn on_audio(id: u32, wv: &ICoreWebView2) {
    let (mut playing, mut muted) = (BOOL(0), BOOL(0));
    unsafe {
        if let Ok(w8) = wv.cast::<ICoreWebView2_8>() {
            let _ = w8.IsDocumentPlayingAudio(&mut playing);
            let _ = w8.IsMuted(&mut muted);
        }
    }
    app::with(|s| {
        if let Some(t) = s.tab_mut(id) {
            t.audible = playing.as_bool();
            t.muted = muted.as_bool();
        }
        ui::update_tab_by_id(s, id);
    });
    changed();
}

/// A page (re)loaded: its own volume again, and old media info is gone.
pub fn on_navigated(id: u32, wv: &ICoreWebView2) {
    let vol = app::with(|s| s.tab(id).map(|t| t.volume)).flatten().unwrap_or(100);
    if vol != 100 {
        exec(wv, &format!("window.__suppaVol && window.__suppaVol({})", vol as f64 / 100.0));
    }
}

pub fn on_navigation_starting(id: u32) {
    let had = app::with(|s| s.tab_mut(id).and_then(|t| t.media.take()).is_some()).unwrap_or(false);
    if had {
        changed();
    }
}

/// Player button + open popup follow the tabs.
pub fn changed() {
    sync_bar();
    if OPEN.with(|o| o.get()) && popup::open_key() == "player" {
        popup::refresh(items());
    }
}

pub fn sync_bar() {
    let r = app::with(|s| {
        let any = s.tabs.iter().any(has_media);
        let playing = s.tabs.iter().any(|t| t.audible || t.media.as_ref().is_some_and(|m| m["playing"].as_bool().unwrap_or(false)));
        (s.settings.player_button && any, playing)
    });
    if let Some((show, playing)) = r {
        ui::post(serde_json::json!({ "type": "player", "show": show, "playing": playing }).to_string());
    }
}

fn site_of(url: &str) -> String {
    let h = crate::util::host_of(url);
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

fn items() -> Vec<Item> {
    let frames = FRAMES.with(|f| f.borrow().clone());
    let cards = app::with(|s| {
        let mut list: Vec<&Tab> = s.tabs.iter().filter(|t| has_media(t)).collect();
        // Playing first.
        list.sort_by_key(|t| !(t.audible || t.media.as_ref().is_some_and(|m| m["playing"].as_bool().unwrap_or(false))));
        list.iter()
            .take(4)
            .map(|t| {
                let m = t.media.clone().unwrap_or(serde_json::json!({}));
                let title = m["title"].as_str().filter(|x| !x.is_empty()).map(str::to_string).unwrap_or_else(|| ui::tab_title(t));
                let frame = frames.iter().find(|(id, _)| *id == t.id).map(|(_, f)| f.clone()).unwrap_or_default();
                serde_json::json!({
                    "kind": "player", "tab": t.id, "title": title,
                    "artist": m["artist"], "album": m["album"], "art": m["art"],
                    "playing": m["playing"].as_bool().unwrap_or(t.audible),
                    "pos": m["pos"], "dur": m["dur"], "at": m["at"],
                    "video": m["video"].as_bool().unwrap_or(false), "frame": frame,
                    "next": m["next"], "prev": m["prev"],
                    "vol": t.volume, "muted": t.muted, "site": site_of(&t.url), "fav": t.favicon,
                })
            })
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    if cards.is_empty() {
        return vec![Item::new(0, "Nothing is playing").enabled(false)];
    }
    cards.into_iter().map(Item::card).collect()
}

/// The player button (or Ctrl+Shift+M).
pub fn show_popup() {
    OPEN.with(|o| o.set(true));
    grab_frames();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(app::hwnd()), TIMER_MEDIA, 1_500, None);
    }
    popup::show_cards(
        "player",
        items(),
        ui::menu_anchor(),
        true,
        |pick| {
            OPEN.with(|o| o.set(false));
            FRAMES.with(|f| f.borrow_mut().clear());
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(app::hwnd()), TIMER_MEDIA);
            }
            // A click on the title: go to that tab.
            if let Some(id) = pick.filter(|&p| p > 0) {
                if let Some(i) = app::with(|s| s.tab_index(id as u32)).flatten() {
                    crate::tabs::activate(i);
                }
            }
        },
        |data| {
            let id = data["tab"].as_u64().unwrap_or(0) as u32;
            let action = data["action"].as_str().unwrap_or("");
            match action {
                "volume" => set_volume(id, data["value"].as_f64().unwrap_or(100.0).round().clamp(0.0, 100.0) as u8),
                "mute" => toggle_mute(id),
                a @ ("toggle" | "next" | "prev" | "back10" | "fwd10") => control(id, a),
                "save_video" => crate::savemedia::save_tab(Some(id), false),
                "save_mp3" => crate::savemedia::save_tab(Some(id), true),
                _ => {}
            }
        },
    );
}

/// TIMER_MEDIA: new video previews while the popup is open.
pub fn tick() {
    if !OPEN.with(|o| o.get()) || popup::open_key() != "player" {
        OPEN.with(|o| o.set(false));
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(app::hwnd()), TIMER_MEDIA);
        }
        return;
    }
    grab_frames();
}

fn grab_frames() {
    let list = app::with(|s| {
        s.tabs
            .iter()
            .filter(|t| has_media(t) && t.media.as_ref().is_some_and(|m| m["video"].as_bool().unwrap_or(false)))
            .filter_map(|t| Some((t.id, t.webview.clone()?)))
            .take(4)
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for (id, wv) in list {
        unsafe {
            let _ = wv.ExecuteScript(
                &HSTRING::from("window.__suppaMedia ? window.__suppaMedia('frame') : ''"),
                &ExecuteScriptCompletedHandler::create(Box::new(move |res, json| {
                    if res.is_ok() {
                        let url = serde_json::from_str::<String>(&json).unwrap_or_default();
                        if url.starts_with("data:image/") {
                            let changed = FRAMES.with(|f| {
                                let mut f = f.borrow_mut();
                                f.retain(|(i, _)| *i != id);
                                f.push((id, url));
                                true
                            });
                            if changed && OPEN.with(|o| o.get()) && popup::open_key() == "player" {
                                popup::refresh(items());
                            }
                        }
                    }
                    Ok(())
                })),
            );
        }
    }
}

pub fn control(id: u32, action: &str) {
    let wv = app::with(|s| s.tab(id).and_then(|t| t.webview.clone())).flatten();
    if let Some(wv) = wv {
        exec(&wv, &format!("window.__suppaMedia && window.__suppaMedia({})", serde_json::Value::from(action)));
    }
}

/// This tab's own volume, 0..=100 (the Windows volume stays as it is).
pub fn set_volume(id: u32, vol: u8) {
    let wv = app::with(|s| {
        let t = s.tab_mut(id)?;
        t.volume = vol;
        t.webview.clone()
    })
    .flatten();
    if let Some(wv) = wv {
        exec(&wv, &format!("window.__suppaVol && window.__suppaVol({})", vol as f64 / 100.0));
    }
    app::with(|s| ui::update_tab_by_id(s, id));
    changed();
}

pub fn toggle_mute(id: u32) {
    let wv = app::with(|s| s.tab(id).and_then(|t| t.webview.clone())).flatten();
    let Some(wv) = wv else { return };
    unsafe {
        if let Ok(w8) = wv.cast::<ICoreWebView2_8>() {
            let mut m = BOOL(0);
            let _ = w8.IsMuted(&mut m);
            let _ = w8.SetIsMuted(!m.as_bool());
        }
    }
    // IsMutedChanged follows and updates the tab.
}

pub fn toggle_mute_active() {
    if let Some(id) = app::with(|s| s.active_tab().map(|t| t.id)).flatten() {
        toggle_mute(id);
    }
}

/// Volume submenu of the tab menu.
pub fn volume_items(index: usize) -> Vec<Item> {
    let (vol, muted) = app::with(|s| s.tabs.get(index).map(|t| (t.volume, t.muted))).flatten().unwrap_or((100, false));
    let mut v = vec![Item::new(VOL_MUTE, if muted { "Unmute tab" } else { "Mute tab" }).key("Ctrl+M"), Item::sep()];
    for p in [100u8, 75, 50, 25, 10] {
        v.push(Item::new(VOL_BASE + p as i64, format!("{p}%")).checked(vol == p));
    }
    v
}

pub const VOL_MUTE: i64 = 70_000;
pub const VOL_BASE: i64 = 70_100;

pub fn volume_pick(index: usize, pick: i64) {
    let Some(id) = app::with(|s| s.tabs.get(index).map(|t| t.id)).flatten() else { return };
    if pick == VOL_MUTE {
        toggle_mute(id);
    } else if (VOL_BASE..=VOL_BASE + 100).contains(&pick) {
        set_volume(id, (pick - VOL_BASE) as u8);
    }
}
