// Ad & tracker blocking with Brave's adblock-rust engine (same lists as on Windows).
//
//  * Network blocking: the rules are converted once into WebKit content-blocker
//    rules, which WebKit compiles and applies itself (fast, nothing per request).
//  * Cosmetic filtering: per-site CSS + scriptlets injected when a page starts,
//    plus generic hide rules computed from the page's classes/ids.
//  * Lists are downloaded in the background, compiled once and cached in
//    <data>/filters; refreshed every 4 days.
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::resources::Resource;
use gtk::glib;
use gtk::glib::translate::ToGlibPtr;
use webkit2gtk::UserContentManager;
use webkit2gtk::ffi as wk;

use crate::{app, util};

const LISTS: [(&str, &str); 4] = [
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy.txt", "https://easylist.to/easylist/easyprivacy.txt"),
    ("ublock-filters.txt", "https://ublockorigin.github.io/uAssets/filters/filters.txt"),
    ("cookie-banners.txt", "https://secure.fanboy.co.nz/fanboy-cookiemonster.txt"),
];
const RESOURCES: (&str, &str) =
    ("resources.json", "https://raw.githubusercontent.com/brave/adblock-resources/master/dist/resources.json");
const REFRESH_SECS: u64 = 4 * 24 * 3600;
/// WebKit refuses content-blocker lists above 150 000 rules.
const MAX_RULES: usize = 145_000;
const FILTER_ID: &str = "suppa-adblock";

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
    static STORE: Cell<*mut wk::WebKitUserContentFilterStore> = const { Cell::new(std::ptr::null_mut()) };
    static FILTER: Cell<*mut wk::WebKitUserContentFilter> = const { Cell::new(std::ptr::null_mut()) };
}

/// Loads (or downloads + compiles) the filters on a background thread.
pub fn start(force_update: bool) {
    std::thread::spawn(move || {
        let dir = util::sub_dir("filters");
        let dat = dir.join("engine.dat");
        let cb = dir.join("webkit-rules.json");
        let stale = util::file_age_secs(&dat).map(|a| a > REFRESH_SECS).unwrap_or(true);
        let marker = dir.join("lists.ver");
        let custom = crate::scripts::custom_filters();
        let mut wanted: String = LISTS.iter().map(|(_, u)| *u).collect::<Vec<_>>().join("\n");
        wanted.push_str("\nwebkit-rules:1");
        for (name, text) in &custom {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in text.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            wanted.push_str(&format!("\ncustom:{name}:{h:x}"));
        }
        let changed = std::fs::read_to_string(&marker).map(|m| m != wanted).unwrap_or(true);
        let mut rules_changed = false;

        if force_update || stale || changed || !cb.exists() {
            let bust = util::unix_now();
            std::thread::scope(|sc| {
                for (file, url) in LISTS.iter().chain(std::iter::once(&RESOURCES)) {
                    let (dir, file, url) = (&dir, *file, *url);
                    sc.spawn(move || {
                        if let Err(e) = util::download(&format!("{url}?_={bust}"), &dir.join(file)) {
                            util::log(&format!("adblock: {e}"));
                        }
                    });
                }
            });
            let texts: Vec<String> = LISTS
                .iter()
                .filter_map(|(file, _)| std::fs::read_to_string(dir.join(file)).ok())
                .chain(custom.into_iter().map(|(_, t)| t))
                .collect();
            if !texts.is_empty() {
                // Engine (cosmetic rules) ...
                let mut set = FilterSet::new(false);
                for t in &texts {
                    set.add_filter_list(t.clone(), ParseOptions::default());
                }
                let engine = Engine::new_with_filter_set(set);
                let tmp = dir.join("engine.dat.new");
                let ok = std::fs::write(&tmp, engine.serialize()).is_ok() && std::fs::rename(&tmp, &dat).is_ok();
                // ... and the WebKit network rules.
                let mut set = FilterSet::new(true);
                for t in &texts {
                    set.add_filter_list(t.clone(), ParseOptions::default());
                }
                match set.into_content_blocking() {
                    Ok((mut rules, _)) => {
                        if rules.len() > MAX_RULES {
                            // Keep the exceptions (they come last) so allowed sites stay allowed.
                            let cut = rules.len() - MAX_RULES;
                            rules.drain(MAX_RULES / 2..MAX_RULES / 2 + cut);
                        }
                        let json = serde_json::to_string(&rules).unwrap_or_else(|_| "[]".into());
                        rules_changed = std::fs::write(&cb, json).is_ok();
                        util::log(&format!("adblock: {} WebKit network rules", rules.len()));
                    }
                    Err(()) => util::log("adblock: could not convert the rules for WebKit"),
                }
                if ok {
                    let _ = std::fs::write(&marker, &wanted);
                }
            }
        }

        let bytes = std::fs::read(&dat).ok();
        let resources: Vec<Resource> =
            std::fs::read(dir.join(RESOURCES.0)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        glib::MainContext::default().invoke(move || on_ready(bytes, resources, rules_changed));
    });
}

/// On the main thread once the background loader is finished.
fn on_ready(bytes: Option<Vec<u8>>, resources: Vec<Resource>, rules_changed: bool) {
    if let Some(bytes) = bytes {
        let mut engine = Engine::default();
        if engine.deserialize(&bytes).is_ok() {
            engine.use_resources(resources);
            ENGINE.with(|e| *e.borrow_mut() = Some(engine));
            util::log(&format!("startup: ad blocker ready at {} ms", util::since_start_ms()));
        } else {
            let _ = std::fs::remove_file(util::sub_dir("filters").join("engine.dat"));
        }
    }
    load_webkit_filter(rules_changed);
}

pub fn is_ready() -> bool {
    ENGINE.with(|e| e.borrow().is_some())
}

// ---- WebKit content blocker -----------------------------------------------------------

fn store() -> *mut wk::WebKitUserContentFilterStore {
    STORE.with(|s| {
        if s.get().is_null() {
            let path = util::sub_dir("filters").join("webkit-store");
            let c = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
            s.set(unsafe { wk::webkit_user_content_filter_store_new(c.as_ptr()) });
        }
        s.get()
    })
}

/// Loads the compiled rules (fast), or compiles them from webkit-rules.json.
fn load_webkit_filter(recompile: bool) {
    let id = std::ffi::CString::new(FILTER_ID).unwrap();
    unsafe {
        if recompile {
            compile();
        } else {
            wk::webkit_user_content_filter_store_load(store(), id.as_ptr(), std::ptr::null_mut(), Some(loaded_cb), std::ptr::null_mut());
        }
    }
}

unsafe fn compile() {
    let Ok(json) = std::fs::read(util::sub_dir("filters").join("webkit-rules.json")) else { return };
    let bytes = glib::Bytes::from_owned(json);
    let id = std::ffi::CString::new(FILTER_ID).unwrap();
    let t0 = std::time::Instant::now();
    COMPILE_START.with(|c| c.set(Some(t0)));
    unsafe {
        wk::webkit_user_content_filter_store_save(
            store(),
            id.as_ptr(),
            bytes.to_glib_none().0,
            std::ptr::null_mut(),
            Some(saved_cb),
            std::ptr::null_mut(),
        );
    }
}

thread_local! {
    static COMPILE_START: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
}

unsafe extern "C" fn loaded_cb(src: *mut glib::gobject_ffi::GObject, res: *mut webkit2gtk::gio::ffi::GAsyncResult, _: glib::ffi::gpointer) {
    let mut err: *mut glib::ffi::GError = std::ptr::null_mut();
    let f = unsafe { wk::webkit_user_content_filter_store_load_finish(src as *mut _, res, &mut err) };
    if f.is_null() {
        if !err.is_null() {
            unsafe { glib::ffi::g_error_free(err) };
        }
        unsafe { compile() }; // not compiled yet
        return;
    }
    set_filter(f);
}

unsafe extern "C" fn saved_cb(src: *mut glib::gobject_ffi::GObject, res: *mut webkit2gtk::gio::ffi::GAsyncResult, _: glib::ffi::gpointer) {
    let mut err: *mut glib::ffi::GError = std::ptr::null_mut();
    let f = unsafe { wk::webkit_user_content_filter_store_save_finish(src as *mut _, res, &mut err) };
    if f.is_null() {
        let msg = if err.is_null() {
            "unknown error".to_string()
        } else {
            let m = unsafe { std::ffi::CStr::from_ptr((*err).message) }.to_string_lossy().to_string();
            unsafe { glib::ffi::g_error_free(err) };
            m
        };
        util::log(&format!("adblock: WebKit could not compile the rules: {msg}"));
        return;
    }
    let ms = COMPILE_START.with(|c| c.take()).map(|t| t.elapsed().as_millis()).unwrap_or(0);
    util::log(&format!("adblock: WebKit rules compiled in {ms} ms"));
    set_filter(f);
}

fn set_filter(f: *mut wk::WebKitUserContentFilter) {
    FILTER.with(|c| {
        let old = c.replace(f);
        if !old.is_null() {
            unsafe { wk::webkit_user_content_filter_unref(old) };
        }
    });
    reattach_all();
}

/// Turns network blocking on/off for one page's content manager.
pub fn attach(ucm: &UserContentManager, on: bool) {
    let f = FILTER.with(|c| c.get());
    let ptr: *mut wk::WebKitUserContentManager = ucm.to_glib_none().0;
    unsafe {
        if on && !f.is_null() && app::with(|s| s.settings.adblock).unwrap_or(true) {
            wk::webkit_user_content_manager_add_filter(ptr, f);
        } else {
            wk::webkit_user_content_manager_remove_all_filters(ptr);
        }
    }
}

/// After the rules or the settings changed: every open tab gets them (or not).
pub fn reattach_all() {
    use crate::wk::*;
    let views = app::with(|s| s.tabs.iter().filter_map(|t| t.view.clone()).collect::<Vec<_>>()).unwrap_or_default();
    for v in views {
        let host = util::host_of(&v.uri().map(|u| u.to_string()).unwrap_or_default());
        let on = app::with(|s| s.settings.adblock && !s.settings.is_allowed(&host)).unwrap_or(true);
        if let Some(ucm) = v.user_content_manager() {
            attach(&ucm, on);
        }
    }
}

// ---- Cosmetic filtering ----------------------------------------------------------------

fn css_rules<'a>(selectors: impl IntoIterator<Item = &'a String>) -> String {
    let mut css = String::new();
    for s in selectors {
        css.push_str(s);
        css.push_str("{display:none!important}\n");
    }
    css
}

/// Script for a page at `url`: hides site-specific ad elements and runs
/// anti-ad scriptlets. Empty when nothing applies.
pub fn document_script(url: &str) -> String {
    if !util::is_web_url(url) {
        return String::new();
    }
    ENGINE.with(|e| {
        let e = e.borrow();
        let Some(engine) = e.as_ref() else { return String::new() };
        let res = engine.url_cosmetic_resources(url);
        let css = css_rules(res.hide_selectors.iter());
        if css.is_empty() && res.injected_script.is_empty() {
            return String::new();
        }
        let host = util::host_of(url);
        format!(
            "(function(){{try{{if(location.hostname!=={host})return;}}catch(e){{return;}}\n\
             var css={css};\n\
             if(css){{var add=function(){{var r=document.head||document.documentElement;if(!r)return false;var s=document.createElement('style');s.id='suppa-cosmetic';s.textContent=css;r.appendChild(s);return true;}};\
             if(!add())document.addEventListener('readystatechange',add,{{once:true}});}}\n\
             {script}\n}})();",
            host = util::js_str(&host),
            css = util::js_str(&css),
            script = res.injected_script,
        )
    })
}

/// Collects class names / ids once the DOM is ready and sends them to Suppa
/// so generic "##.ad-banner" style rules can be applied.
pub const COLLECTOR_SCRIPT: &str = r#"(function(){
if(window.top!==window)return;
function run(){try{
 var c=new Set(),i=new Set(),els=document.querySelectorAll('[id],[class]');
 for(var k=0;k<els.length&&k<20000;k++){var e=els[k];if(e.id)i.add(e.id);if(e.classList)for(var j=0;j<e.classList.length;j++)c.add(e.classList[j]);}
 window.webkit.messageHandlers.suppa.postMessage(JSON.stringify({t:'suppa-cos',c:Array.from(c).slice(0,5000),i:Array.from(i).slice(0,5000)}));
}catch(e){}}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',run,{once:true});else run();
})();"#;

/// Returns JS that injects generic hide rules for the given page, or empty.
pub fn generic_hide_script(url: &str, message_json: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(message_json) else { return String::new() };
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
        format!(
            "(function(){{var s=document.createElement('style');s.textContent={};(document.head||document.documentElement).appendChild(s);}})();",
            util::js_str(&css_rules(sels.iter()))
        )
    })
}
