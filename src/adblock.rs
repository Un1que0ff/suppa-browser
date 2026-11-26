// Ad & tracker blocking using Brave's adblock-rust engine.
//
//  * Network blocking: every request goes through `check()` (microseconds).
//  * Cosmetic filtering: per-site CSS + scriptlets injected at document start,
//    plus generic hide rules computed from the page's classes/ids.
//  * Filter lists are downloaded in the background, compiled once and cached
//    as engine.dat (fast startup); refreshed every 4 days.
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Mutex;

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::resources::Resource;
use adblock::Engine;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::Foundation::HWND;

use crate::app::WM_APP_ADBLOCK_READY;
use crate::util;

// Add or remove lists here; the cache rebuilds automatically when this changes.
const LISTS: [(&str, &str); 5] = [
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy.txt", "https://easylist.to/easylist/easyprivacy.txt"),
    ("ublock-filters.txt", "https://ublockorigin.github.io/uAssets/filters/filters.txt"),
    // uBlock Origin's fast-moving fixes (YouTube changes its ads every few weeks)
    ("ublock-quick-fixes.txt", "https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt"),
    ("cookie-banners.txt", "https://secure.fanboy.co.nz/fanboy-cookiemonster.txt"),
];
const RESOURCES: (&str, &str) = (
    "resources.json",
    "https://raw.githubusercontent.com/brave/adblock-resources/master/dist/resources.json",
);
const REFRESH_SECS: u64 = 4 * 24 * 3600;

static PENDING: Mutex<Option<(Vec<u8>, Vec<Resource>)>> = Mutex::new(None);

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

/// Loads (or downloads + compiles) the filter engine on a background thread.
/// Posts WM_APP_ADBLOCK_READY to `hwnd` when done.
pub fn start(hwnd: HWND, force_update: bool) {
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let dir = util::sub_dir("filters");
        let dat = dir.join("engine.dat");
        let stale = util::file_age_secs(&dat).map(|a| a > REFRESH_SECS).unwrap_or(true);
        let marker = dir.join("lists.ver");
        // Your own rules from scripts\filters\*.txt are part of the cache key,
        // so editing them rebuilds the engine on the next start.
        let custom = crate::scripts::custom_filters();
        let mut wanted: String = LISTS.iter().map(|(_, u)| *u).collect::<Vec<_>>().join("\n");
        for (name, text) in &custom {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in text.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            wanted.push_str(&format!("\ncustom:{name}:{h:x}"));
        }
        let changed = std::fs::read_to_string(&marker).map(|m| m != wanted).unwrap_or(true);

        if force_update || stale || changed {
            // All lists download at the same time (one thread each).
            let bust = util::unix_now();
            std::thread::scope(|sc| {
                for (file, url) in LISTS.iter().chain(std::iter::once(&RESOURCES)) {
                    let (dir, file, url) = (&dir, *file, *url);
                    sc.spawn(move || {
                        unsafe {
                            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
                        }
                        let _ = util::download(&format!("{url}?_={bust}"), &dir.join(file));
                    });
                }
            });

            let mut set = FilterSet::new(false);
            let mut any = false;
            for (file, _) in LISTS {
                if let Ok(text) = std::fs::read_to_string(dir.join(file)) {
                    set.add_filter_list(text, ParseOptions::default());
                    any = true;
                }
            }
            for (_, text) in custom {
                set.add_filter_list(text, ParseOptions::default());
            }
            if any {
                let engine = Engine::new_with_filter_set(set);
                // Write to a temp file first so a crash can't leave a half-written cache.
                let tmp = dir.join("engine.dat.new");
                let ok = std::fs::write(&tmp, engine.serialize()).is_ok()
                    && std::fs::rename(&tmp, &dat).is_ok();
                if ok {
                    let _ = std::fs::write(&marker, &wanted);
                } else {
                    util::log(&format!("adblock: could not write {}", dat.display()));
                }
            }
        }

        let Ok(bytes) = std::fs::read(&dat) else { return };
        let resources: Vec<Resource> = std::fs::read(dir.join(RESOURCES.0))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        *PENDING.lock().unwrap() = Some((bytes, resources));
        util::post(HWND(hwnd_raw as *mut _), WM_APP_ADBLOCK_READY, 0, 0);
    });
}

/// Called on the UI thread when the background loader is finished.
pub fn on_ready() {
    let Some((bytes, resources)) = PENDING.lock().unwrap().take() else { return };
    let t0 = std::time::Instant::now();
    let mut engine = Engine::default();
    if engine.deserialize(&bytes).is_err() {
        // Corrupt/old cache: delete so the next start rebuilds it.
        let _ = std::fs::remove_file(util::sub_dir("filters").join("engine.dat"));
        return;
    }
    engine.use_resources(resources);
    ENGINE.with(|e| *e.borrow_mut() = Some(engine));
    util::log(&format!(
        "startup: ad blocker ready at {} ms (loading it took {} ms on the UI thread)",
        crate::util::since_start_ms(),
        t0.elapsed().as_millis()
    ));
}

pub fn is_ready() -> bool {
    ENGINE.with(|e| e.borrow().is_some())
}

fn context_name(ctx: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> &'static str {
    match ctx {
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => "subdocument",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST => "xmlhttprequest",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "fetch",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT => "csp_report",
        _ => "other",
    }
}

pub enum Verdict {
    Allow,
    Block,
    /// Serve this body (mime, bytes) instead of the real resource.
    Redirect(String, Vec<u8>),
}

/// Decide what to do with a network request made by a page at `source_url`.
pub fn check(url: &str, source_url: &str, ctx: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> Verdict {
    if !util::is_web_url(url) {
        return Verdict::Allow;
    }
    ENGINE.with(|e| {
        let e = e.borrow();
        let Some(engine) = e.as_ref() else { return Verdict::Allow };
        let source = if util::is_web_url(source_url) { source_url } else { url };
        let Ok(req) = Request::new(url, source, context_name(ctx), "get") else {
            return Verdict::Allow;
        };
        let r = engine.check_network_request(&req);
        if !r.should_block() {
            return Verdict::Allow;
        }
        if let Some(body) = r.redirect {
            if let Some(parsed) = parse_data_url(&body) {
                return Verdict::Redirect(parsed.0, parsed.1);
            }
        }
        Verdict::Block
    })
}

fn parse_data_url(s: &str) -> Option<(String, Vec<u8>)> {
    use base64::Engine as _;
    let rest = s.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if let Some(mime) = meta.strip_suffix(";base64") {
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
        Some((mime.to_string(), bytes))
    } else {
        Some((meta.to_string(), data.as_bytes().to_vec()))
    }
}

fn css_rules<'a>(selectors: impl IntoIterator<Item = &'a String>) -> String {
    // One rule per selector: an invalid selector then only drops itself.
    let mut css = String::new();
    for s in selectors {
        css.push_str(s);
        css.push_str("{display:none!important}\n");
    }
    css
}

/// Script injected at document start for a navigation to `url`: hides
/// site-specific ad elements and runs anti-ad scriptlets (e.g. YouTube).
/// Empty when nothing applies.
pub fn document_script(url: &str) -> String {
    if !util::is_web_url(url) {
        return String::new();
    }
    ENGINE.with(|e| {
        let e = e.borrow();
        let Some(engine) = e.as_ref() else { return String::new() };
        let res = engine.url_cosmetic_resources(url);
        let css = css_rules(res.hide_selectors.iter());
        let host = util::host_of(url);
        // YouTube: Suppa's own ad skipper as well (scripts\youtube.js).
        let yt = if is_youtube(&host) { crate::scripts::youtube_script() } else { "" };
        if css.is_empty() && res.injected_script.is_empty() && yt.is_empty() {
            return String::new();
        }
        format!(
            "(function(){{try{{if(location.hostname!=={host})return;}}catch(e){{return;}}\n\
             var css={css};\n\
             if(css){{var add=function(){{var r=document.head||document.documentElement;if(!r)return false;var s=document.createElement('style');s.id='suppa-cosmetic';s.textContent=css;r.appendChild(s);return true;}};\
             if(!add())document.addEventListener('readystatechange',add,{{once:true}});}}\n\
             {script}\n}})();\n{yt}",
            host = util::js_str(&host),
            css = util::js_str(&css),
            script = res.injected_script,
            yt = yt,
        )
    })
}

pub fn is_youtube(host: &str) -> bool {
    host == "youtube.com" || host.ends_with(".youtube.com")
}

/// The YouTube ad skipper for a page that has finished loading (in case the
/// per-page script was registered too late for it; it runs only once per page).
pub fn youtube_late_script(url: &str) -> Option<&'static str> {
    (util::is_web_url(url) && is_youtube(&util::host_of(url)) && is_ready()).then(crate::scripts::youtube_script)
}

/// Collects class names / ids once the DOM is ready and sends them to the host
/// so generic "##.ad-banner" style rules can be applied.
pub const COLLECTOR_SCRIPT: &str = r#"(function(){
if(window.top!==window)return;
function run(){try{
 var c=new Set(),i=new Set(),els=document.querySelectorAll('[id],[class]');
 for(var k=0;k<els.length&&k<20000;k++){var e=els[k];if(e.id)i.add(e.id);if(e.classList)for(var j=0;j<e.classList.length;j++)c.add(e.classList[j]);}
 window.chrome.webview.postMessage(JSON.stringify({t:'suppa-cos',c:Array.from(c).slice(0,5000),i:Array.from(i).slice(0,5000)}));
}catch(e){}}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',run,{once:true});else run();
})();"#;

/// Returns JS that injects generic hide rules for the given page, or empty.
pub fn generic_hide_script(url: &str, message_json: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(message_json) else { return String::new() };
    if v.get("t").and_then(|t| t.as_str()) != Some("suppa-cos") {
        return String::new();
    }
    let list = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).take(5000).collect())
            .unwrap_or_default()
    };
    let classes = list("c");
    let ids = list("i");
    ENGINE.with(|e| {
        let e = e.borrow();
        let Some(engine) = e.as_ref() else { return String::new() };
        let res = engine.url_cosmetic_resources(url);
        if res.generichide {
            return String::new();
        }
        let exceptions: HashSet<String> = res.exceptions;
        let sels = engine.hidden_class_id_selectors(&classes, &ids, &exceptions);
        if sels.is_empty() {
            return String::new();
        }
        let css = css_rules(sels.iter());
        format!(
            "(function(){{var s=document.createElement('style');s.textContent={};(document.head||document.documentElement).appendChild(s);}})();",
            util::js_str(&css)
        )
    })
}
