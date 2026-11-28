// Suppa's own vault: saved addresses, payment cards, PayPal accounts and the
// passwords Suppa generated (or got from Dropbox). Kept in vault.dat in the data
// folder, encrypted by Windows for this user account (DPAPI).
//
//  * Pages get a small key button in address, card and password fields
//    (scripts\forms.js). Clicking it opens Suppa's own menu (never page code)
//    with what fits; only the entry you pick is filled into the page.
//  * Password fields for a new password offer a strong generated password; it
//    is filled in, saved here and put into the engine's password store.
//  * Cards are filled only after Windows Hello / the account password. Security
//    codes (CVC) are never stored.
//  * Settings -> Addresses and payments / Passwords show and edit everything.
//
// Every entry is a JSON object with "id", "t" (changed, ms) and "del" (deleted,
// kept so Dropbox sync can pass deletions on). See sync.rs.
use std::sync::Mutex;

use base64::Engine;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::Foundation::{HLOCAL, LocalFree, POINT};
use windows::Win32::Security::Cryptography::*;
use windows::core::HSTRING;

use crate::popup::{self, Item};
use crate::{app, util};

pub type Obj = serde_json::Map<String, serde_json::Value>;

pub const KINDS: [&str; 4] = ["logins", "addresses", "cards", "paypal"];

#[derive(Default, Clone)]
pub struct Vault {
    pub lists: [Vec<Obj>; 4],
}

impl Vault {
    pub fn list(&self, kind: &str) -> &Vec<Obj> {
        &self.lists[kind_index(kind)]
    }
    pub fn list_mut(&mut self, kind: &str) -> &mut Vec<Obj> {
        &mut self.lists[kind_index(kind)]
    }
    pub fn to_json(&self) -> serde_json::Value {
        let mut m = Obj::new();
        for (i, k) in KINDS.iter().enumerate() {
            m.insert(k.to_string(), serde_json::Value::Array(self.lists[i].iter().cloned().map(serde_json::Value::Object).collect()));
        }
        serde_json::Value::Object(m)
    }
    pub fn from_json(v: &serde_json::Value) -> Vault {
        let mut out = Vault::default();
        for (i, k) in KINDS.iter().enumerate() {
            if let Some(a) = v[k].as_array() {
                out.lists[i] = a.iter().filter_map(|x| x.as_object().cloned()).filter(|o| !s(o, "id").is_empty()).collect();
            }
        }
        out
    }
}

fn kind_index(kind: &str) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(1)
}

/// A string field of an entry.
pub fn s(o: &Obj, k: &str) -> String {
    o.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}
pub fn t(o: &Obj) -> i64 {
    o.get("t").and_then(|v| v.as_i64()).unwrap_or(0)
}
pub fn deleted(o: &Obj) -> bool {
    o.get("del").and_then(|v| v.as_bool()).unwrap_or(false)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

static VAULT: Mutex<Option<Vault>> = Mutex::new(None);

/// Runs `f` with the vault (loaded from disk the first time). Any thread.
pub fn with<R>(f: impl FnOnce(&mut Vault) -> R) -> R {
    let mut g = VAULT.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(load());
    }
    f(g.as_mut().unwrap())
}

/// Changes the vault, writes it and schedules a Dropbox sync.
pub fn change<R>(f: impl FnOnce(&mut Vault) -> R) -> R {
    let r = with(|v| {
        let r = f(v);
        save(v);
        r
    });
    crate::sync::changed();
    r
}

fn path() -> std::path::PathBuf {
    util::data_dir().join("vault.dat")
}

fn load() -> Vault {
    let Ok(raw) = std::fs::read(path()) else { return Vault::default() };
    let Some(plain) = unprotect(&raw) else {
        util::log("vault: could not be opened (other Windows account?)");
        return Vault::default();
    };
    serde_json::from_slice::<serde_json::Value>(&plain).map(|v| Vault::from_json(&v)).unwrap_or_default()
}

pub fn save(v: &Vault) {
    let text = v.to_json().to_string();
    match protect(text.as_bytes()) {
        Some(b) => {
            let tmp = path().with_extension("tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(&tmp, path());
            }
        }
        None => util::log("vault: could not encrypt"),
    }
}

// ---- Windows encryption for this account (DPAPI) ---------------------------------------

pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(&input, &HSTRING::from("Suppa vault"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).ok()?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Some(v)
    }
}

pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).ok()?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Some(v)
    }
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    unsafe {
        let _ = BCryptGenRandom(None, &mut b, BCRYPT_USE_SYSTEM_PREFERRED_RNG);
    }
    b
}

fn new_id() -> String {
    random_bytes(9).iter().map(|b| format!("{b:02x}")).collect()
}

// ---- strong passwords --------------------------------------------------------------------

const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const LOWER: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
const DIGIT: &[u8] = b"23456789";
const SYMBOL: &[u8] = b"!@#$%^&*-_=+?";

/// A random password like "A%$3G&8^ga12Kq": upper and lower case letters,
/// digits and symbols, at least two of each (letters that look alike are left out).
pub fn generate(len: usize) -> String {
    let len = len.clamp(8, 64);
    let all: Vec<u8> = [UPPER, LOWER, DIGIT, SYMBOL].concat();
    // An unbiased random index below n (rejection sampling on bytes).
    let mut pool = Vec::new();
    let mut pick = |n: usize| -> usize {
        let limit = 256 - 256 % n;
        loop {
            if pool.is_empty() {
                pool = random_bytes(64);
            }
            let b = pool.pop().unwrap() as usize;
            if b < limit {
                return b % n;
            }
        }
    };
    loop {
        let pw: Vec<u8> = (0..len).map(|_| all[pick(all.len())]).collect();
        let count = |set: &[u8]| pw.iter().filter(|c| set.contains(c)).count();
        let need = if len >= 12 { 2 } else { 1 };
        if [UPPER, LOWER, DIGIT, SYMBOL].iter().all(|set| count(set) >= need) {
            return String::from_utf8(pw).unwrap_or_default();
        }
    }
}

// ---- entries -----------------------------------------------------------------------------

/// Login id: the same site and user name is one entry everywhere (engine store,
/// generated, Dropbox).
pub fn login_id(origin: &str, user: &str) -> String {
    format!("{origin}\u{1}{user}")
}

/// Saves a login (new or changed password). Returns true when something changed.
pub fn put_login(origin: &str, user: &str, password: &str, generated: bool) -> bool {
    let id = login_id(origin, user);
    change(|v| {
        let list = v.list_mut("logins");
        if let Some(o) = list.iter_mut().find(|o| s(o, "id") == id) {
            if !deleted(o) && s(o, "password") == password {
                return false;
            }
            o.insert("password".into(), password.into());
            o.insert("del".into(), false.into());
            o.insert("t".into(), now_ms().into());
            return true;
        }
        let mut o = Obj::new();
        o.insert("id".into(), id.into());
        o.insert("origin".into(), origin.into());
        o.insert("user".into(), user.into());
        o.insert("password".into(), password.into());
        o.insert("generated".into(), generated.into());
        o.insert("t".into(), now_ms().into());
        list.push(o);
        true
    })
}

/// Marks a login deleted (also passed on to Dropbox).
pub fn forget_login(origin: &str, user: &str) {
    let id = login_id(origin, user);
    change(|v| {
        if let Some(o) = v.list_mut("logins").iter_mut().find(|o| s(o, "id") == id) {
            o.insert("del".into(), true.into());
            o.insert("password".into(), "".into());
            o.insert("t".into(), now_ms().into());
        }
    });
}

/// Entries of one kind that are not deleted.
pub fn live(kind: &str) -> Vec<Obj> {
    with(|v| v.list(kind).iter().filter(|o| !deleted(o)).cloned().collect())
}

fn digits(x: &str) -> String {
    x.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// "•••• 1234"
pub fn card_label(o: &Obj) -> String {
    let n = digits(&s(o, "number"));
    let last = if n.len() >= 4 { &n[n.len() - 4..] } else { n.as_str() };
    let brand = card_brand(&n);
    format!("{brand} •••• {last}")
}

pub fn card_brand(n: &str) -> &'static str {
    let p2: u32 = n.get(..2).and_then(|x| x.parse().ok()).unwrap_or(0);
    let p4: u32 = n.get(..4).and_then(|x| x.parse().ok()).unwrap_or(0);
    match n.chars().next() {
        Some('4') => "Visa",
        Some('5') if (51..=55).contains(&p2) => "Mastercard",
        Some('2') if (2221..=2720).contains(&p4) => "Mastercard",
        Some('3') if p2 == 34 || p2 == 37 => "Amex",
        Some('6') => "Discover",
        _ => "Card",
    }
}

fn address_label(o: &Obj) -> String {
    let parts: Vec<String> = ["name", "street1", "city"].iter().map(|k| s(o, k)).filter(|x| !x.is_empty()).collect();
    let label = s(o, "label");
    if label.is_empty() { parts.join(", ") } else { format!("{label} - {}", parts.join(", ")) }
}

const ADDRESS_FIELDS: [&str; 12] =
    ["label", "name", "company", "street1", "street2", "city", "region", "zip", "country", "phone", "email", "id"];
const CARD_FIELDS: [&str; 5] = ["label", "name", "number", "exp", "id"];

/// Clean copy of an entry the Settings page or a form sent.
fn clean(kind: &str, item: &serde_json::Value) -> Option<Obj> {
    let fields: &[&str] = match kind {
        "addresses" => &ADDRESS_FIELDS,
        "cards" => &CARD_FIELDS,
        "paypal" => &["email", "name", "id"],
        _ => return None,
    };
    let mut o = Obj::new();
    for f in fields {
        let v = item.get(*f).and_then(|x| x.as_str()).unwrap_or("").trim().chars().take(200).collect::<String>();
        o.insert(f.to_string(), v.into());
    }
    if kind == "cards" {
        let n = digits(&s(&o, "number"));
        if n.len() < 12 || n.len() > 19 {
            return None;
        }
        o.insert("number".into(), n.into());
    }
    if kind == "paypal" && !s(&o, "email").contains('@') {
        return None;
    }
    if kind == "addresses" && ["name", "street1", "city", "zip", "email", "phone"].iter().all(|k| s(&o, k).is_empty()) {
        return None;
    }
    Some(o)
}

/// Adds or updates an entry (Settings page). Returns false if it was not valid.
pub fn put(kind: &str, item: &serde_json::Value) -> bool {
    let Some(mut o) = clean(kind, item) else { return false };
    if s(&o, "id").is_empty() {
        o.insert("id".into(), new_id().into());
    }
    o.insert("t".into(), now_ms().into());
    let id = s(&o, "id");
    change(|v| {
        let list = v.list_mut(kind);
        if let Some(x) = list.iter_mut().find(|x| s(x, "id") == id) {
            // A card edited without a new number keeps its number.
            *x = o;
        } else {
            list.push(o);
        }
    });
    true
}

pub fn delete(kind: &str, id: &str) {
    change(|v| {
        if let Some(o) = v.list_mut(kind).iter_mut().find(|o| s(o, "id") == id) {
            let keep = ["id"];
            o.retain(|k, _| keep.contains(&k.as_str()));
            o.insert("del".into(), true.into());
            o.insert("t".into(), now_ms().into());
        }
    });
}

/// The lists for Settings. Card numbers and passwords are never in it.
pub fn list_json() -> serde_json::Value {
    let masked: Vec<serde_json::Value> = live("cards")
        .iter()
        .map(|o| serde_json::json!({ "id": s(o, "id"), "label": s(o, "label"), "name": s(o, "name"), "exp": s(o, "exp"), "masked": card_label(o) }))
        .collect();
    let logins: Vec<serde_json::Value> = live("logins")
        .iter()
        .map(|o| {
            let origin = s(o, "origin");
            serde_json::json!({ "id": s(o, "id"), "origin": origin, "site": util::host_of(&origin).trim_start_matches("www.").to_string(), "user": s(o, "user"), "generated": o.get("generated").and_then(|x| x.as_bool()).unwrap_or(false) })
        })
        .collect();
    serde_json::json!({
        "type": "wallet",
        "addresses": live("addresses"),
        "cards": masked,
        "paypal": live("paypal"),
        "logins": logins,
        "sync": crate::sync::status_json(),
    })
}

/// Full entry (card number, password) for Settings after Windows Hello.
pub fn secret(kind: &str, id: &str) -> Option<String> {
    let o = live(kind).into_iter().find(|o| s(o, "id") == id)?;
    Some(match kind {
        "cards" => s(&o, "number"),
        "logins" => s(&o, "password"),
        _ => return None,
    })
}

// ---- filling pages -----------------------------------------------------------------------

/// Where to run the fill script: the page itself, or the frame (iframe) that asked.
#[derive(Clone)]
pub enum Target {
    Page(ICoreWebView2),
    Frame(ICoreWebView2Frame2),
}

impl Target {
    pub fn exec(&self, js: &str) {
        unsafe {
            match self {
                Target::Page(w) => {
                    let _ = w.ExecuteScript(&HSTRING::from(js), None::<&ICoreWebView2ExecuteScriptCompletedHandler>);
                }
                Target::Frame(f) => {
                    let _ = f.ExecuteScript(&HSTRING::from(js), None::<&ICoreWebView2ExecuteScriptCompletedHandler>);
                }
            }
        }
    }
}

const PICK_ADDRESS: i64 = 1_000;
const PICK_CARD: i64 = 2_000;
const PICK_PAYPAL: i64 = 3_000;
const PICK_LOGIN: i64 = 4_000;
const PICK_GENERATE: i64 = 9_001;
const PICK_MANAGE: i64 = 9_002;
const PICK_PAYPAL_BUTTON: i64 = 9_003;
const PICK_MANAGE_PW: i64 = 9_004;

fn fill(target: &Target, kind: &str, data: serde_json::Value) {
    target.exec(&format!("window.__suppaFill && window.__suppaFill({}, {})", serde_json::Value::from(kind), data));
}

/// The key button in a field was clicked (scripts\forms.js), from page / frame
/// `source` (its address). `msg`: {kind, user, paypal (PayPal option on the page)}.
pub fn on_fill_request(target: Target, source: &str, msg: &serde_json::Value, incognito: bool) {
    if !app::with(|s| s.settings.fill_forms).unwrap_or(true) {
        return;
    }
    let kind = msg["kind"].as_str().unwrap_or("").to_string();
    let user_typed = msg["user"].as_str().unwrap_or("").to_string();
    let Some(origin) = crate::passwords::origin(source) else { return };
    let mut items = Vec::new();
    let addresses = live("addresses");
    let cards = live("cards");
    let paypal = live("paypal");
    let logins: Vec<Obj> = live("logins").into_iter().filter(|o| s(o, "origin") == origin).collect();
    let mut generated = String::new();
    match kind.as_str() {
        "address" => {
            for (i, a) in addresses.iter().enumerate() {
                items.push(Item::new(PICK_ADDRESS + i as i64, address_label(a)));
            }
            if addresses.is_empty() {
                items.push(Item::new(0, "No saved addresses yet").enabled(false));
            }
        }
        "card" => {
            for (i, c) in cards.iter().enumerate() {
                let label = s(c, "label");
                let name = if label.is_empty() { s(c, "name") } else { label };
                items.push(Item::new(PICK_CARD + i as i64, card_label(c)).key(&format!("{}  {}", name, s(c, "exp"))));
            }
            if cards.is_empty() {
                items.push(Item::new(0, "No saved cards yet").enabled(false));
            }
            if msg["paypal"].as_bool().unwrap_or(false) {
                items.push(Item::sep());
                items.push(Item::new(PICK_PAYPAL_BUTTON, "Pay with PayPal instead"));
            }
        }
        "paypal" => {
            for (i, p) in paypal.iter().enumerate() {
                items.push(Item::new(PICK_PAYPAL + i as i64, s(p, "email")).key("PayPal"));
            }
            if paypal.is_empty() {
                items.push(Item::new(0, "No saved PayPal accounts yet").enabled(false));
            }
        }
        "login" | "newpw" => {
            if kind == "newpw" {
                generated = generate(16);
                items.push(Item::new(PICK_GENERATE, format!("Use a strong password:  {generated}")).hot(true));
                items.push(Item::new(0, "Suppa saves it for this site").enabled(false));
            }
            for (i, l) in logins.iter().enumerate() {
                let u = s(l, "user");
                items.push(Item::new(PICK_LOGIN + i as i64, if u.is_empty() { "(no user name)".to_string() } else { u }).key("••••••••"));
            }
            if logins.is_empty() && kind == "login" {
                items.push(Item::new(0, "No Suppa passwords for this site").enabled(false));
            }
        }
        _ => return,
    }
    items.push(Item::sep());
    if kind == "login" || kind == "newpw" {
        items.push(Item::new(PICK_MANAGE_PW, "Manage passwords..."));
    } else {
        items.push(Item::new(PICK_MANAGE, "Manage addresses and payments..."));
    }
    // Under the mouse (the key button was just clicked).
    let mut pt = POINT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
    }
    pt.y += 12;
    popup::show("fill", items, pt, false, move |pick| {
        let Some(id) = pick else { return };
        match id {
            PICK_MANAGE => crate::tabs::open_settings_at("wallet"),
            PICK_MANAGE_PW => crate::tabs::open_settings_at("passwords"),
            PICK_PAYPAL_BUTTON => target.exec("window.__suppaFill && window.__suppaFill('paypal_button', {})"),
            PICK_GENERATE => {
                fill(&target, "newpw", serde_json::json!({ "password": generated }));
                if !incognito {
                    save_generated(&origin, &user_typed, &generated);
                }
            }
            i if (PICK_LOGIN..PICK_LOGIN + 900).contains(&i) => {
                if let Some(l) = logins.get((i - PICK_LOGIN) as usize) {
                    fill(&target, "login", serde_json::json!({ "user": s(l, "user"), "password": s(l, "password") }));
                }
            }
            i if (PICK_ADDRESS..PICK_ADDRESS + 900).contains(&i) => {
                if let Some(a) = addresses.get((i - PICK_ADDRESS) as usize) {
                    fill(&target, "address", serde_json::Value::Object(a.clone()));
                }
            }
            i if (PICK_PAYPAL..PICK_PAYPAL + 900).contains(&i) => {
                if let Some(p) = paypal.get((i - PICK_PAYPAL) as usize) {
                    fill(&target, "paypal", serde_json::json!({ "email": s(p, "email") }));
                }
            }
            i if (PICK_CARD..PICK_CARD + 900).contains(&i) => {
                let Some(c) = cards.get((i - PICK_CARD) as usize).cloned() else { return };
                crate::auth::require("Suppa wants to fill in a saved card", move |ok| {
                    if ok {
                        fill(&target, "card", serde_json::json!({ "name": s(&c, "name"), "number": s(&c, "number"), "exp": s(&c, "exp") }));
                    }
                });
            }
            _ => {}
        }
    });
}

/// A generated password was used: saved here and put into the engine's own
/// store (so the engine fills it in too, from the next start).
fn save_generated(origin: &str, user: &str, password: &str) {
    put_login(origin, user, password, true);
    remember_recent(origin, user);
    if let Err(e) = crate::passwords::queue_logins(&[(origin.to_string(), user.to_string(), password.to_string())]) {
        util::log(&format!("generated password: not queued for the engine: {e}"));
    }
    util::log(&format!("generated password saved for {}", util::host_of(origin)));
}

thread_local! {
    /// The last generated password: (origin, user name at the time, when).
    static RECENT: std::cell::RefCell<Option<(String, String, std::time::Instant)>> = const { std::cell::RefCell::new(None) };
}

fn remember_recent(origin: &str, user: &str) {
    RECENT.with(|r| *r.borrow_mut() = Some((origin.to_string(), user.to_string(), std::time::Instant::now())));
}

/// A form with a password was sent (scripts\forms.js): if its password was just
/// generated without a user name, the user name typed meanwhile is added.
pub fn on_login_submit(source: &str, user: &str) {
    let Some(origin) = crate::passwords::origin(source) else { return };
    let recent = RECENT.with(|r| r.borrow().clone());
    let Some((o, old_user, when)) = recent else { return };
    if o != origin || old_user == user || user.is_empty() || when.elapsed().as_secs() > 1800 {
        return;
    }
    let old = login_id(&origin, &old_user);
    let pw = with(|v| v.list("logins").iter().find(|x| s(x, "id") == old && !deleted(x)).map(|x| s(x, "password")));
    let Some(pw) = pw else { return };
    forget_login(&origin, &old_user);
    put_login(&origin, user, &pw, true);
    remember_recent(&origin, user);
    let _ = crate::passwords::queue_logins(&[(origin.clone(), user.to_string(), pw)]);
}

/// A form with an address or card was sent: offer to save what is new.
pub fn on_form_saved(msg: &serde_json::Value, incognito: bool) {
    let on = app::with(|s| s.settings.offer_save && s.settings.fill_forms).unwrap_or(false);
    if !on || incognito {
        return;
    }
    if let Some(card) = msg.get("card").and_then(|c| clean("cards", c)) {
        let n = s(&card, "number");
        if !live("cards").iter().any(|c| s(c, "number") == n) {
            let label = card_label(&card);
            let item = serde_json::Value::Object(card);
            popup::confirm(&format!("Save {label} in Suppa?"), "Next time Suppa fills it in for you (after Windows Hello). The security code (CVC) is never saved.", "Save card", "Not now", move || {
                put("cards", &item);
            });
            return;
        }
    }
    if let Some(addr) = msg.get("address").and_then(|a| clean("addresses", a)) {
        let same = live("addresses").iter().any(|a| {
            ["name", "street1", "zip", "email"].iter().all(|k| s(a, k).eq_ignore_ascii_case(&s(&addr, k)))
        });
        if !same && !s(&addr, "street1").is_empty() {
            let label = address_label(&addr);
            let item = serde_json::Value::Object(addr);
            popup::confirm("Save this address in Suppa?", &label, "Save address", "Not now", move || {
                put("addresses", &item);
            });
        }
    }
}

/// Settings page messages: {cmd:"wallet", op, kind, id, item}.
pub fn on_settings_message(wv: &ICoreWebView2, v: &serde_json::Value) {
    let st = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let (op, kind, id) = (st("op"), st("kind"), st("id"));
    let send = |wv: &ICoreWebView2, j: serde_json::Value| unsafe {
        let _ = wv.PostWebMessageAsJson(&HSTRING::from(j.to_string()));
    };
    match op.as_str() {
        "list" => send(wv, list_json()),
        "save" => {
            let ok = put(&kind, v.get("item").unwrap_or(&serde_json::Value::Null));
            if !ok {
                send(wv, serde_json::json!({ "type": "pw_toast", "text": "Please fill in the fields first" }));
            }
            send(wv, list_json());
        }
        "delete" => {
            if kind == "logins" {
                if let Some(o) = live("logins").into_iter().find(|o| s(o, "id") == id) {
                    forget_login(&s(&o, "origin"), &s(&o, "user"));
                }
            } else {
                delete(&kind, &id);
            }
            send(wv, list_json());
        }
        "generate" => {
            let len = v.get("len").and_then(|x| x.as_u64()).unwrap_or(16) as usize;
            send(wv, serde_json::json!({ "type": "generated", "password": generate(len) }));
        }
        op @ ("reveal" | "copy") => {
            let wv = wv.clone();
            let what = if kind == "cards" { "card number" } else { "password" };
            let reason = format!("Suppa wants to {} a saved {what}", if op == "reveal" { "show" } else { "copy" });
            let op = op.to_string();
            crate::auth::require(&reason, move |ok| {
                let reply = match (ok, secret(&kind, &id)) {
                    (false, _) => serde_json::json!({ "type": "pw_toast", "text": "Not confirmed - it stays hidden" }),
                    (true, None) => serde_json::json!({ "type": "pw_toast", "text": "Not found" }),
                    (true, Some(x)) if op == "reveal" => serde_json::json!({ "type": "wallet_reveal", "kind": kind, "id": id, "value": x }),
                    (true, Some(x)) => {
                        let ok = util::set_clipboard(&x);
                        serde_json::json!({ "type": "pw_toast", "text": if ok { "Copied" } else { "Could not copy" } })
                    }
                };
                unsafe {
                    let _ = wv.PostWebMessageAsJson(&HSTRING::from(reply.to_string()));
                }
            });
        }
        "sync_passphrase" => {
            let text = match crate::sync::set_passphrase(&st("value")) {
                Ok(()) => "Passphrase saved. Syncing...".to_string(),
                Err(e) => e,
            };
            send(wv, serde_json::json!({ "type": "pw_toast", "text": text }));
            send(wv, list_json());
        }
        "sync_now" => util::post(app::hwnd(), crate::app::WM_APP_COMMAND, crate::app::CMD_SYNC_NOW as usize, 0),
        "sync_folder" => util::post(app::hwnd(), crate::app::WM_APP_COMMAND, crate::app::CMD_SYNC_FOLDER as usize, 0),
        _ => {}
    }
}

/// ≡ -> Generate strong password: copied to the clipboard.
pub fn copy_generated() {
    let pw = generate(16);
    if util::set_clipboard(&pw) {
        crate::backup::show_notice("✓  Strong password copied", &format!("{pw}\n\nPaste it where you sign up. In password fields, Suppa's key button fills and saves one for you."));
    }
}

#[allow(dead_code)]
pub fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}
