// Saves CPU and RAM by putting background tabs to sleep.
//
//  1. Hidden tabs immediately get Visible=false + low memory target
//     (Chromium throttles timers and trims memory).
//  2. After `sleep_minutes` idle they are suspended (0% CPU).
//  3. After `discard_minutes` idle they are unloaded completely (renderer
//     process freed). Clicking the tab reloads it.
// Tabs playing audio, and tabs set to "Keep awake" (tab menu, ≡ -> Page), are
// never put to sleep.
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::TrySuspendCompletedHandler;
use windows::core::{BOOL, Interface};

use crate::app;
use crate::ui;

enum Action {
    Suspend(u32, ICoreWebView2),
    Discard(u32, ICoreWebView2Controller, ICoreWebView2),
}

fn playing_audio(wv: &ICoreWebView2) -> bool {
    unsafe {
        let Ok(w8) = wv.cast::<ICoreWebView2_8>() else { return false };
        let mut b = BOOL(0);
        let _ = w8.IsDocumentPlayingAudio(&mut b);
        b.as_bool()
    }
}

/// Called every 30 s by a timer. `force` = sleep all background tabs now.
pub fn tick(force: bool) {
    let now = Instant::now();
    let actions = app::with(|s| {
        let sleep = Duration::from_secs(s.settings.sleep_minutes * 60);
        let discard = Duration::from_secs(s.settings.discard_minutes * 60);
        let mut out = Vec::new();
        for (i, t) in s.tabs.iter().enumerate() {
            if (i == s.active && !s.minimized) || t.awake {
                continue;
            }
            let (Some(c), Some(wv)) = (&t.controller, &t.webview) else { continue };
            let idle = now.saturating_duration_since(t.last_active);
            if i != s.active && s.settings.discard_minutes > 0 && idle >= discard {
                out.push(Action::Discard(t.id, c.clone(), wv.clone()));
            } else if !t.suspended && (force || idle >= sleep) && i != s.active {
                out.push(Action::Suspend(t.id, wv.clone()));
            }
        }
        out
    })
    .unwrap_or_default();

    for a in actions {
        match a {
            Action::Suspend(id, wv) => {
                if playing_audio(&wv) {
                    continue;
                }
                let Ok(w3) = wv.cast::<ICoreWebView2_3>() else { continue };
                unsafe {
                    let _ = w3.TrySuspend(&TrySuspendCompletedHandler::create(Box::new(move |res, ok| {
                        if res.is_ok() && ok {
                            app::with(|s| {
                                if let Some(t) = s.tab_mut(id) {
                                    t.suspended = true;
                                }
                                ui::update_tab_by_id(s, id);
                            });
                        }
                        Ok(())
                    })));
                }
            }
            Action::Discard(id, c, wv) => {
                if playing_audio(&wv) {
                    continue;
                }
                app::with(|s| {
                    if let Some(t) = s.tab_mut(id) {
                        t.controller = None;
                        t.webview = None;
                        t.suspended = false;
                        t.loading = false;
                        t.doc_script_id = None;
                    }
                    ui::update_tab_by_id(s, id);
                });
                unsafe {
                    let _ = c.Close();
                }
            }
        }
    }
}
