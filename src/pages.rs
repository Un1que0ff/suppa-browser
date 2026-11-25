// Suppa's own pages besides Settings: History (https://suppa.history/ =
// scripts\history.html) and Downloads (https://suppa.downloads/ =
// scripts\downloads.html), plus messages from the start page (most-used
// shortcuts). Served by the request interceptor in webview.rs like Settings.
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::core::HSTRING;

use crate::{app, util};

pub const HISTORY_URL: &str = "https://suppa.history/";

/// host -> page file in the scripts folder.
pub fn file_for(host: &str) -> Option<&'static str> {
    match host {
        "suppa.history" => Some("history.html"),
        "suppa.downloads" => Some("downloads.html"),
        _ => None,
    }
}

/// Hosts whose pages may talk to Suppa.
pub fn is_page(host: &str) -> bool {
    file_for(host).is_some() || host == crate::newtab::HOST
}

fn reply(wv: &ICoreWebView2, v: &serde_json::Value) {
    unsafe {
        let _ = wv.PostWebMessageAsJson(&HSTRING::from(v.to_string()));
    }
}

pub fn on_message(wv: &ICoreWebView2, host: &str, json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let n = |k: &str| v.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
    match (host, s("cmd").as_str()) {
        // Theme and colours, the same message the Settings page gets.
        (_, "get") => crate::settings_page::send_state(wv),
        ("suppa.history", "list") => reply(wv, &crate::history::list_json(&s("q"), n("offset") as usize, 200)),
        ("suppa.history", "delete") => {
            let list: Vec<(i64, String)> = v["items"]
                .as_array()
                .map(|a| a.iter().map(|x| (x["t"].as_i64().unwrap_or(0), x["url"].as_str().unwrap_or("").to_string())).collect())
                .unwrap_or_default();
            crate::history::delete(&list);
            reply(wv, &serde_json::json!({ "type": "hist_changed" }));
        }
        ("suppa.history", "clear") => {
            let since = n("since");
            let wv = wv.clone();
            let what = if since > 0 { "the history of the last hour" } else { "all history" };
            app::later(move || {
                crate::popup::confirm(&format!("Delete {what}?"), "Pages you visited are removed from Suppa's history and the start page shortcuts.", "Delete", "Cancel", move || {
                    crate::history::clear(since);
                    reply(&wv, &serde_json::json!({ "type": "hist_changed" }));
                });
            });
        }
        ("suppa.downloads", "list") => reply(wv, &crate::downloads::list_json()),
        ("suppa.downloads", "dl") => {
            // Posted: opening files and dialogs never run inside the WebView event.
            let op = s("op");
            let id = n("id") as u64;
            crate::app::later(move || crate::downloads::on_page(&op, id));
        }
        (crate::newtab::HOST, "hide_site") => crate::history::hide_site(&s("host")),
        _ => {}
    }
}

/// Sends a message to every open tab showing `host`.
pub fn broadcast(host: &str, v: &serde_json::Value) {
    let wvs = app::with(|s| s.tabs.iter().filter(|t| util::host_of(&t.url) == host).filter_map(|t| t.webview.clone()).collect::<Vec<_>>())
        .unwrap_or_default();
    for w in &wvs {
        reply(w, v);
    }
}
