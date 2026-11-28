// Sync and share passwords, addresses, cards and PayPal accounts through Dropbox
// (Settings -> Passwords -> Sync with Dropbox; off until you turn it on).
//
// Suppa writes one file, <Dropbox>\Apps\Suppa\suppa-vault.json, through the
// Dropbox app on this PC (Suppa itself never talks to Dropbox's servers and needs
// no Dropbox login). The file is encrypted with a passphrase you choose:
// PBKDF2-SHA256 (310 000 rounds) -> AES-256-GCM, both from Windows (CNG), so
// Dropbox and anyone without the passphrase see only random bytes.
//
// Every PC with the same Dropbox folder and passphrase gets the same passwords,
// so does anyone you share that Dropbox folder with ("sharing"). Changes are
// merged per entry (the newer one wins, deletions are passed on); passwords from
// other PCs go into the engine's own password store at the next start.
// Another folder (OneDrive, a USB stick...) can be chosen too.
use std::path::PathBuf;
use std::sync::Mutex;

use base64::Engine;
use windows::Win32::Security::Cryptography::*;

use crate::app::{self, *};
use crate::util;
use crate::vault::{self, Obj, Vault, deleted, s, t};

const ROUNDS: u64 = 310_000;
const FILE: &str = "suppa-vault.json";

struct State {
    status: String,
    ok: bool,
    last: Option<std::time::SystemTime>,
    running: bool,
    /// Modified time of the Dropbox file when it was last read or written.
    seen: Option<std::time::SystemTime>,
    /// (salt, key) of the last derivation (PBKDF2 is slow on purpose).
    key: Option<(Vec<u8>, Vec<u8>)>,
}

static STATE: Mutex<State> = Mutex::new(State { status: String::new(), ok: true, last: None, running: false, seen: None, key: None });

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap_or_else(|e| e.into_inner()))
}

fn enabled() -> bool {
    app::with(|s| s.settings.sync_on).unwrap_or(false)
}

/// The Dropbox folder of this PC (from the Dropbox app's info.json).
pub fn dropbox_dir() -> Option<PathBuf> {
    for var in ["LOCALAPPDATA", "APPDATA"] {
        let Ok(base) = std::env::var(var) else { continue };
        let Ok(text) = std::fs::read_to_string(PathBuf::from(base).join("Dropbox").join("info.json")) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        for acc in ["personal", "business"] {
            if let Some(p) = v[acc]["path"].as_str() {
                if std::path::Path::new(p).is_dir() {
                    return Some(PathBuf::from(p));
                }
            }
        }
    }
    let home = PathBuf::from(std::env::var("USERPROFILE").ok()?).join("Dropbox");
    home.is_dir().then_some(home)
}

/// Folder that holds the sync file.
fn folder() -> Option<PathBuf> {
    let custom = app::with(|s| s.settings.sync_folder.clone()).unwrap_or_default();
    if !custom.is_empty() {
        return Some(PathBuf::from(custom));
    }
    dropbox_dir().map(|d| d.join("Apps").join("Suppa"))
}

fn file() -> Option<PathBuf> {
    folder().map(|f| f.join(FILE))
}

fn key_path() -> PathBuf {
    util::data_dir().join("sync.key")
}

fn passphrase() -> Option<String> {
    let raw = std::fs::read(key_path()).ok()?;
    String::from_utf8(vault::unprotect(&raw)?).ok().filter(|p| !p.is_empty())
}

/// Settings: the passphrase for the Dropbox file (kept encrypted for this account).
pub fn set_passphrase(p: &str) -> Result<(), String> {
    if p.chars().count() < 8 {
        return Err("Use at least 8 characters for the passphrase".into());
    }
    let enc = vault::protect(p.as_bytes()).ok_or("Windows could not encrypt the passphrase")?;
    std::fs::write(key_path(), enc).map_err(|e| e.to_string())?;
    st(|s| s.key = None);
    sync_now();
    Ok(())
}

/// For Settings: on/off, folder, last result.
pub fn status_json() -> serde_json::Value {
    let (status, ok, last, running) = st(|s| (s.status.clone(), s.ok, s.last, s.running));
    let ago = last.and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    serde_json::json!({
        "on": enabled(),
        "folder": folder().map(|f| f.display().to_string()).unwrap_or_default(),
        "dropbox": dropbox_dir().is_some(),
        "custom": !app::with(|s| s.settings.sync_folder.clone()).unwrap_or_default().is_empty(),
        "has_pass": passphrase().is_some(),
        "status": status, "ok": ok, "ago": ago, "running": running,
    })
}

// ---- crypto --------------------------------------------------------------------------------

fn derive(pass: &str, salt: &[u8]) -> Result<Vec<u8>, String> {
    if let Some(k) = st(|s| s.key.clone().filter(|(sa, _)| sa == salt).map(|(_, k)| k)) {
        return Ok(k);
    }
    let mut key = vec![0u8; 32];
    unsafe {
        let mut alg = BCRYPT_ALG_HANDLE::default();
        BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_SHA256_ALGORITHM, None, BCRYPT_ALG_HANDLE_HMAC_FLAG).ok().map_err(|e| e.to_string())?;
        let r = BCryptDeriveKeyPBKDF2(alg, Some(pass.as_bytes()), Some(salt), ROUNDS, &mut key, 0).ok();
        let _ = BCryptCloseAlgorithmProvider(alg, 0);
        r.map_err(|e| e.to_string())?;
    }
    st(|s| s.key = Some((salt.to_vec(), key.clone())));
    Ok(key)
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}
fn unb64(x: &serde_json::Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(x.as_str().unwrap_or("")).unwrap_or_default()
}

fn encrypt(pass: &str, salt: &[u8], plain: &[u8]) -> Result<String, String> {
    let key = derive(pass, salt)?;
    let nonce: [u8; 12] = vault::random_bytes(12).try_into().map_err(|_| "random")?;
    let (mut ct, tag) = crate::passwords::aes_gcm(&key, &nonce, plain, true, None)?;
    ct.extend_from_slice(&tag);
    Ok(serde_json::json!({
        "suppa": "vault", "v": 1, "kdf": "pbkdf2-sha256", "rounds": ROUNDS,
        "salt": b64(salt), "nonce": b64(&nonce), "data": b64(&ct),
    })
    .to_string())
}

/// -> (salt, plain JSON)
fn decrypt(pass: &str, text: &str) -> Result<(Vec<u8>, serde_json::Value), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|_| "The Dropbox file is damaged".to_string())?;
    let salt = unb64(&v["salt"]);
    let nonce: [u8; 12] = unb64(&v["nonce"]).try_into().map_err(|_| "The Dropbox file is damaged")?;
    let data = unb64(&v["data"]);
    if data.len() < 16 || salt.is_empty() {
        return Err("The Dropbox file is damaged".into());
    }
    let key = derive(pass, &salt)?;
    let (ct, tag) = data.split_at(data.len() - 16);
    let (plain, _) = crate::passwords::aes_gcm(&key, &nonce, ct, false, Some(tag))
        .map_err(|_| "Wrong passphrase: the Dropbox file was made with another one".to_string())?;
    let json = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
    Ok((salt, json))
}

// ---- merge -------------------------------------------------------------------------------

/// Newer entry wins, per id. Returns the logins that came (new or changed) from `remote`.
fn merge(local: &mut Vault, remote: &Vault) -> Vec<Obj> {
    let mut came = Vec::new();
    for (i, kind) in vault::KINDS.iter().enumerate() {
        for r in &remote.lists[i] {
            let id = s(r, "id");
            match local.lists[i].iter_mut().find(|l| s(l, "id") == id) {
                Some(l) if t(r) > t(l) => {
                    *l = r.clone();
                    if *kind == "logins" {
                        came.push(r.clone());
                    }
                }
                Some(_) => {}
                None => {
                    local.lists[i].push(r.clone());
                    if *kind == "logins" {
                        came.push(r.clone());
                    }
                }
            }
        }
    }
    came
}

/// Passwords the engine saved itself (sign-in prompts, imports) go into the
/// vault, so they are synced too.
fn adopt_engine_logins(v: &mut Vault) {
    let Ok(list) = crate::passwords::read_store() else { return };
    for e in list {
        let Some(pw) = e.password else { continue };
        let Some(origin) = crate::passwords::origin(&e.realm) else { continue };
        let id = vault::login_id(&origin, &e.username);
        let logins = v.list_mut("logins");
        match logins.iter_mut().find(|o| s(o, "id") == id) {
            // Deleted on another PC (or here) stays deleted.
            Some(o) if deleted(o) => {}
            Some(o) => {
                if s(o, "password") != pw && !waiting_for_engine(&origin, &e.username) {
                    o.insert("password".into(), pw.into());
                    o.insert("t".into(), vault::now_ms().into());
                }
            }
            None => {
                let mut o = Obj::new();
                o.insert("id".into(), id.into());
                o.insert("origin".into(), origin.into());
                o.insert("user".into(), e.username.into());
                o.insert("password".into(), pw.into());
                // Old: a newer value from another PC wins.
                o.insert("t".into(), 1.into());
                logins.push(o);
            }
        }
    }
}

/// A login waiting in passwords-import.json for the next start.
fn waiting_for_engine(origin: &str, user: &str) -> bool {
    std::fs::read_to_string(util::data_dir().join("passwords-import.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["items"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .any(|it| it["origin"].as_str() == Some(origin) && it["username"].as_str() == Some(user))
}

// ---- running -------------------------------------------------------------------------------

/// The vault changed: sync a few seconds later (if sync is on).
pub fn changed() {
    if enabled() {
        // Posted: this can run on the sync thread, and timers belong to the UI thread.
        util::post(app::hwnd(), WM_APP_COMMAND, CMD_SYNC_SOON as usize, 0);
    }
}

/// CMD_SYNC_SOON (UI thread): sync 3 s after the last change.
pub fn schedule() {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(app::hwnd()), TIMER_SYNC, 3_000, None);
    }
}

/// Suppa got the focus again: sync if the Dropbox file changed meanwhile.
pub fn check_remote() {
    if !enabled() {
        return;
    }
    let Some(f) = file() else { return };
    let m = std::fs::metadata(&f).and_then(|m| m.modified()).ok();
    if m.is_some() && m != st(|s| s.seen) {
        sync_now();
    }
}

/// Syncs now, in the background (CMD_SYNC_NOW, the timer, start).
pub fn sync_now() {
    if !enabled() || st(|s| std::mem::replace(&mut s.running, true)) {
        return;
    }
    let Some(file) = file() else {
        finish(false, "No Dropbox folder found on this PC. Install the Dropbox app, or choose a folder.".into());
        return;
    };
    let Some(pass) = passphrase() else {
        finish(false, "Choose a passphrase to start syncing".into());
        return;
    };
    std::thread::spawn(move || {
        let r = run(&file, &pass);
        match r {
            Ok(msg) => finish(true, msg),
            Err(e) => finish(false, e),
        }
    });
}

fn finish(ok: bool, msg: String) {
    if !ok {
        util::log(&format!("sync: {msg}"));
    }
    st(|s| {
        s.running = false;
        s.ok = ok;
        s.status = msg;
        s.last = Some(std::time::SystemTime::now());
    });
    util::post(app::hwnd(), WM_APP_COMMAND, CMD_SYNC_DONE as usize, 0);
}

fn run(file: &std::path::Path, pass: &str) -> Result<String, String> {
    let _ = std::fs::create_dir_all(file.parent().unwrap_or(file));
    // 1. What the other PCs wrote.
    let (salt, remote) = match std::fs::read_to_string(file) {
        Ok(text) => {
            let (salt, json) = decrypt(pass, &text)?;
            (salt, Some(Vault::from_json(&json)))
        }
        Err(_) => (vault::random_bytes(16), None),
    };
    // 2. Merge into this PC's vault (and the engine's own passwords).
    let mut came = Vec::new();
    let merged = vault::with(|v| {
        adopt_engine_logins(v);
        if let Some(r) = &remote {
            came = merge(v, r);
        }
        vault::save(v);
        v.clone()
    });
    // 3. Passwords from other PCs -> the engine's store (next start); deletions too.
    let add: Vec<(String, String, String)> =
        came.iter().filter(|o| !deleted(o)).map(|o| (s(o, "origin"), s(o, "user"), s(o, "password"))).collect();
    if !add.is_empty() {
        if let Err(e) = crate::passwords::queue_logins(&add) {
            util::log(&format!("sync: passwords not queued for the engine: {e}"));
        }
    }
    for o in came.iter().filter(|o| deleted(o)) {
        let (origin, user) = o.get("id").and_then(|x| x.as_str()).and_then(|id| id.split_once('\u{1}')).unwrap_or(("", ""));
        if !origin.is_empty() {
            crate::passwords::delete(&format!("{origin}/"), user);
        }
    }
    // 4. Write the merged result back.
    let text = encrypt(pass, &salt, merged.to_json().to_string().as_bytes())?;
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("Could not write to the Dropbox folder: {e}"))?;
    std::fs::rename(&tmp, file).map_err(|e| format!("Could not write to the Dropbox folder: {e}"))?;
    let seen = std::fs::metadata(file).and_then(|m| m.modified()).ok();
    st(|s| s.seen = seen);
    let count = |k: &str| merged.list(k).iter().filter(|o| !deleted(o)).count();
    let mut msg = format!(
        "Synced: {} passwords, {} addresses, {} cards, {} PayPal",
        count("logins"),
        count("addresses"),
        count("cards"),
        count("paypal")
    );
    if !add.is_empty() {
        msg += &format!(" - {} new from other PCs (in the browser's own list after a restart)", add.len());
    }
    Ok(msg)
}

/// CMD_SYNC_DONE: Settings pages show the result.
pub fn on_done() {
    crate::settings_page::refresh_wallet();
}

/// Settings -> Choose folder.
pub fn choose_folder() {
    if let Some(p) = crate::extensions::pick_folder_titled("Folder to sync Suppa's passwords with (e.g. in Dropbox)") {
        app::with(|s| {
            s.settings.sync_folder = p.display().to_string();
            crate::settings::save(&s.settings);
        });
        st(|s| s.seen = None);
        sync_now();
        crate::settings_page::refresh_wallet();
    }
}
