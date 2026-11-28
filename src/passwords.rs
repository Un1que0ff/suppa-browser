// Import saved passwords from Firefox (or Chrome / Edge) via their CSV export.
//
//   Firefox: about:logins -> ... -> Export passwords   (url, username, password, ...)
//   Chrome / Edge: Passwords -> Export                  (name, url, username, password, note)
//
// WebView2 has no API to add passwords, so they go straight into the engine's
// own password store (UserData\EBWebView\Default\Login Data, an SQLite file),
// written the way the engine writes them itself:
//   * password_value = "v10" + 12-byte nonce + AES-256-GCM(password) + 16-byte tag
//   * the AES key is in "Local State" (os_crypt.encrypted_key), protected by
//     Windows for this user account (DPAPI)
//   * Edge also keeps a row in logins_edge_extended with the same id.
// The engine keeps that file locked while it runs, so the passwords are
// encrypted right away (no plain text is written anywhere), kept in
// passwords-import.json, and put in when Suppa starts again, before the engine
// opens the file. SQLite is the copy that ships with Windows (winsqlite3.dll).
use std::path::{Path, PathBuf};

use base64::Engine;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::*;

use crate::sqlite::Sqlite;
use crate::util;

const PENDING: &str = "passwords-import.json";
const RESULT: &str = "passwords-import-result.txt";

fn profile_dir() -> PathBuf {
    util::sub_dir("UserData").join("EBWebView")
}

// ---- CSV -------------------------------------------------------------------------------

/// Splits CSV text into records (quotes, "" inside quotes, line breaks inside quotes).
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// One login from the file.
struct Login {
    origin: String, // "https://site.com" (no slash)
    action: String,
    username: String,
    password: String,
    created_ms: Option<u64>,
}

/// "https://www.site.com:8443/path?x" -> "https://www.site.com:8443"
fn origin_of(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    let host = host.rsplit('@').next()?.to_string(); // drop user:pass@
    (!host.is_empty()).then(|| format!("{scheme}://{host}"))
}

/// Logins from a Firefox / Chrome / Edge password CSV, and how many rows were skipped.
fn read_logins(text: &str) -> Result<(Vec<Login>, usize), String> {
    let rows = parse_csv(text);
    let Some(head) = rows.first() else { return Err("The file is empty.".into()) };
    let col = |name: &str| head.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let (Some(url), Some(user), Some(pass)) = (col("url"), col("username"), col("password")) else {
        return Err("This is not a password export: it needs the columns url, username and password.".into());
    };
    let realm = col("httpRealm");
    let action = col("formActionOrigin");
    let created = col("timeCreated");
    let mut out = Vec::new();
    let mut skipped = 0;
    for r in &rows[1..] {
        let get = |i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
        let password = i_get(r, pass);
        // HTTP sign-in boxes (httpRealm) and non-web entries are left out.
        let Some(origin) = origin_of(&get(Some(url))) else {
            skipped += 1;
            continue;
        };
        if password.is_empty() || !get(realm).is_empty() {
            skipped += 1;
            continue;
        }
        let action = origin_of(&get(action)).unwrap_or_else(|| origin.clone());
        out.push(Login { origin, action, username: get(Some(user)), password, created_ms: get(created).parse().ok() });
    }
    Ok((out, skipped))
}

/// The password exactly as written (spaces at the ends are part of it).
fn i_get(r: &[String], i: usize) -> String {
    r.get(i).cloned().unwrap_or_default()
}

// ---- encryption (same as the engine: DPAPI-protected key, AES-256-GCM) -----------------

/// The engine's password key from "Local State".
fn engine_key() -> Result<Vec<u8>, String> {
    let text = std::fs::read_to_string(profile_dir().join("Local State"))
        .map_err(|_| "Suppa's password store is not set up yet. Open any website once, then try again.".to_string())?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("Local State: {e}"))?;
    let b64 = json["os_crypt"]["encrypted_key"].as_str().ok_or("The engine has no password key yet. Open any website once, then try again.")?;
    let raw = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| format!("password key: {e}"))?;
    let blob = raw.strip_prefix(b"DPAPI").ok_or("The engine uses a password key format Suppa does not know.")?;
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: blob.len() as u32, pbData: blob.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output).map_err(|e| format!("Windows could not unlock the password key: {e}"))?;
        let key = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        if key.len() != 32 {
            return Err("The engine's password key has an unexpected size.".into());
        }
        Ok(key)
    }
}

/// AES-256-GCM with Windows' own crypto (CNG). Returns (ciphertext, tag).
pub fn aes_gcm(key: &[u8], nonce: &[u8; 12], data: &[u8], encrypt: bool, tag_in: Option<&[u8]>) -> Result<(Vec<u8>, [u8; 16]), String> {
    unsafe {
        let mut alg = BCRYPT_ALG_HANDLE::default();
        BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_AES_ALGORITHM, None, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0)).ok().map_err(|e| e.to_string())?;
        let mode: Vec<u8> = "ChainingModeGCM\0".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let result = (|| {
            BCryptSetProperty(alg.into(), BCRYPT_CHAINING_MODE, &mode, 0).ok().map_err(|e| e.to_string())?;
            let mut hkey = BCRYPT_KEY_HANDLE::default();
            BCryptGenerateSymmetricKey(alg, &mut hkey, None, key, 0).ok().map_err(|e| e.to_string())?;
            let mut tag = [0u8; 16];
            if let Some(t) = tag_in {
                tag.copy_from_slice(&t[..16]);
            }
            let info = BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
                cbSize: std::mem::size_of::<BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO>() as u32,
                dwInfoVersion: 1, // BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION
                pbNonce: nonce.as_ptr() as *mut u8,
                cbNonce: 12,
                pbTag: tag.as_mut_ptr(),
                cbTag: 16,
                ..Default::default()
            };
            let mut out = vec![0u8; data.len()];
            let mut n = 0u32;
            let st = if encrypt {
                BCryptEncrypt(hkey, Some(data), Some(&info as *const _ as *const _), None, Some(&mut out), &mut n, BCRYPT_FLAGS(0))
            } else {
                BCryptDecrypt(hkey, Some(data), Some(&info as *const _ as *const _), None, Some(&mut out), &mut n, BCRYPT_FLAGS(0))
            };
            let _ = BCryptDestroyKey(hkey);
            st.ok().map_err(|e| e.to_string())?;
            out.truncate(n as usize);
            Ok((out, tag))
        })();
        let _ = BCryptCloseAlgorithmProvider(alg, 0);
        result
    }
}

/// "v10" + nonce + ciphertext + tag, like the engine stores a password.
fn encrypt_password(key: &[u8], password: &str) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 12];
    unsafe {
        BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG).ok().map_err(|e| e.to_string())?;
    }
    let (ct, tag) = aes_gcm(key, &nonce, password.as_bytes(), true, None)?;
    // Check it opens again with the same key before trusting it.
    let (back, _) = aes_gcm(key, &nonce, &ct, false, Some(&tag))?;
    if back != password.as_bytes() {
        return Err("encryption check failed".into());
    }
    let mut v = b"v10".to_vec();
    v.extend_from_slice(&nonce);
    v.extend_from_slice(&ct);
    v.extend_from_slice(&tag);
    Ok(v)
}

// ---- step 1 (while Suppa runs): read the file, encrypt, schedule ------------------------

/// ≡ -> Backup and import -> Import passwords from a file.
pub fn import_file() {
    if let Some(file) = util::open_dialog("Import passwords (Firefox, Chrome or Edge export)", "Password export (.csv)", "*.csv") {
        import_path(file);
    }
}

/// `Suppa.exe --import-passwords <file.csv>`: the same import, file already chosen
/// (the questions are still asked).
pub fn import_from_args() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--import-passwords") {
        if let Some(f) = args.get(i + 1) {
            import_path(PathBuf::from(f));
        }
    }
}

fn import_path(file: PathBuf) {
    let hwnd = crate::app::hwnd();
    util::log(&format!("password import: reading {}", file.display()));
    let text = match std::fs::read(&file) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => {
            util::info(hwnd, &format!("Could not read the file\n\n{e}"));
            return;
        }
    };
    let (logins, skipped) = match read_logins(&text) {
        Ok(x) => x,
        Err(e) => {
            util::info(hwnd, &e);
            return;
        }
    };
    util::log(&format!("password import: {} logins, {skipped} skipped", logins.len()));
    if logins.is_empty() {
        util::info(hwnd, "No website passwords found in that file.");
        return;
    }
    let key = match engine_key() {
        Ok(k) => k,
        Err(e) => {
            util::info(hwnd, &format!("Could not import passwords\n\n{e}"));
            return;
        }
    };
    let mut items = Vec::new();
    for l in &logins {
        match encrypt_password(&key, &l.password) {
            Ok(enc) => items.push(serde_json::json!({
                "origin": l.origin, "action": l.action, "username": l.username,
                "password": base64::engine::general_purpose::STANDARD.encode(enc),
                "created_ms": l.created_ms,
            })),
            Err(e) => {
                util::info(hwnd, &format!("Could not encrypt the passwords\n\n{e}"));
                return;
            }
        }
    }
    let n = items.len();
    let pending = serde_json::json!({ "items": items }).to_string();
    let skipped_note = if skipped > 0 { format!(" ({skipped} entries without a website or password are left out)") } else { String::new() };
    crate::popup::confirm(
        &format!("Import {n} passwords?"),
        &format!(
            "They are added to Suppa's saved passwords{skipped_note}; passwords you already have for the same site and user name are updated. Suppa restarts to do this and reopens your tabs."
        ),
        "Import and restart",
        "Cancel",
        move || {
            if let Err(e) = std::fs::write(util::data_dir().join(PENDING), &pending) {
                util::info(crate::app::hwnd(), &format!("Could not prepare the import\n\n{e}"));
                return;
            }
            // The export holds every password as plain text: offer to delete it.
            // Either way Suppa then restarts and the import runs.
            let f = file.clone();
            crate::popup::confirm_choice(
                "Delete the exported file?",
                &format!("{}\nholds your passwords as plain text. Suppa has what it needs; delete the file now?", f.display()),
                "Delete it",
                "Keep it",
                move |delete| {
                    if delete {
                        let _ = std::fs::remove_file(&f);
                        util::log("password import: export file deleted");
                    }
                    util::log("password import scheduled; restarting");
                    crate::window::restart_now();
                },
            );
        },
    );
}

// ---- step 2 (next start, before the engine): write into the store ----------------------

/// At startup, before the engine opens its files: delete the passwords removed
/// in Settings, then put scheduled imports in.
pub fn apply_pending() {
    let db = profile_dir().join("Default").join("Login Data");
    let del = util::data_dir().join(DELETE);
    if del.exists() {
        match delete_logins(&db, &pending_deletes()) {
            Ok(n) => util::log(&format!("passwords: {n} deleted")),
            Err(e) => util::log(&format!("passwords: delete failed: {e}")),
        }
        let _ = std::fs::remove_file(&del);
    }
    let pending = util::data_dir().join(PENDING);
    let Ok(text) = std::fs::read_to_string(&pending) else { return };
    let msg = match write_logins(&db, &text) {
        Ok((added, updated)) => {
            let _ = std::fs::remove_file(&pending);
            util::log(&format!("password import: {added} added, {updated} updated"));
            format!("ok\n{added}\n{updated}")
        }
        Err(e) => {
            // Kept for the next start only if the store does not exist yet.
            if db.exists() {
                let _ = std::fs::remove_file(&pending);
            }
            util::log(&format!("password import failed: {e}"));
            format!("error\n{e}")
        }
    };
    let _ = std::fs::write(util::data_dir().join(RESULT), msg);
}

/// After the start: tell how the import went (once).
pub fn show_result() {
    let path = util::data_dir().join(RESULT);
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let _ = std::fs::remove_file(&path);
    let mut l = text.lines();
    if l.next() == Some("ok") {
        let added: usize = l.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let updated: usize = l.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let detail = if updated > 0 { format!("{added} new, {updated} updated") } else { format!("{added} new") };
        crate::backup::show_notice("✓  Passwords imported", &format!("{detail}. They fill in on their websites."));
    } else {
        let e = l.collect::<Vec<_>>().join(" ");
        crate::backup::show_notice("Passwords were not imported", &e);
    }
}

/// Windows microseconds (since 1601) for a Unix time in milliseconds.
fn win_time(unix_ms: u64) -> i64 {
    (unix_ms as i64) * 1000 + 11_644_473_600_000_000
}

fn write_logins(db: &Path, json: &str) -> Result<(usize, usize), String> {
    if !db.exists() {
        return Err("Suppa's password store does not exist yet; the import runs on the next start.".into());
    }
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let items = v["items"].as_array().cloned().unwrap_or_default();
    let sql = Sqlite::load()?;
    let conn = sql.open(db)?;
    // Only a store layout we know (the engine's own columns) is written to.
    let cols = conn.query_text("SELECT group_concat(name) FROM pragma_table_info('logins')")?;
    for need in ["origin_url", "signon_realm", "username_value", "password_value", "date_created", "date_password_modified"] {
        if !cols.split(',').any(|c| c == need) {
            return Err(format!("The engine's password store changed (no {need}); nothing was imported."));
        }
    }
    let edge = conn.query_text("SELECT count(*) FROM sqlite_master WHERE name='logins_edge_extended'")? == "1";
    let now_ms = util::unix_now() * 1000;
    conn.exec("BEGIN IMMEDIATE")?;
    let result = (|| {
        let (mut added, mut updated) = (0, 0);
        for it in &items {
            let origin = it["origin"].as_str().unwrap_or_default();
            let action = it["action"].as_str().unwrap_or(origin);
            let user = it["username"].as_str().unwrap_or_default();
            let pw = base64::engine::general_purpose::STANDARD.decode(it["password"].as_str().unwrap_or_default()).map_err(|e| e.to_string())?;
            let realm = format!("{origin}/");
            let created = win_time(it["created_ms"].as_u64().unwrap_or(now_ms));
            let now = win_time(now_ms);
            // Same site + user name already saved: new password.
            let mut st = conn.prepare("UPDATE logins SET password_value=?1, date_password_modified=?2 WHERE signon_realm=?3 AND username_value=?4")?;
            st.blob(1, &pw)?;
            st.int(2, now)?;
            st.text(3, &realm)?;
            st.text(4, user)?;
            st.run()?;
            drop(st);
            if conn.changes() > 0 {
                updated += 1;
                continue;
            }
            let mut st = conn.prepare(
                "INSERT INTO logins (origin_url, action_url, username_element, username_value, password_element, password_value, \
                 submit_element, signon_realm, date_created, blacklisted_by_user, scheme, password_type, times_used, form_data, \
                 display_name, icon_url, federation_url, skip_zero_click, generation_upload_status, possible_username_pairs, \
                 date_last_used, moving_blocked_for, date_password_modified, sender_email, sender_name, date_received, \
                 sharing_notification_displayed, keychain_identifier, sender_profile_image_url, date_last_filled, actor_login_approved) \
                 VALUES (?1, ?2, '', ?3, '', ?4, '', ?5, ?6, 0, 0, 0, 0, x'', '', '', '', 0, 0, x'00000000', 0, x'00000000', ?6, '', '', 0, 0, x'', '', 0, 0)",
            )?;
            st.text(1, &realm)?;
            st.text(2, &format!("{action}/"))?;
            st.text(3, user)?;
            st.blob(4, &pw)?;
            st.text(5, &realm)?;
            st.int(6, created)?;
            st.run()?;
            drop(st);
            if edge {
                let id = conn.last_id();
                let mut st = conn.prepare("INSERT OR IGNORE INTO logins_edge_extended (id) VALUES (?1)")?;
                st.int(1, id)?;
                st.run()?;
            }
            added += 1;
        }
        Ok((added, updated))
    })();
    match result {
        Ok(x) => {
            conn.exec("COMMIT")?;
            Ok(x)
        }
        Err(e) => {
            let _ = conn.exec("ROLLBACK");
            Err(e)
        }
    }
}

// ---- the saved passwords (Settings -> Passwords) ---------------------------------------
//
// The engine keeps its store locked while it runs, so the list is read from a
// copy of the file (it only holds the encrypted values), and deleting is
// scheduled like an import: done when Suppa starts again.

const DELETE: &str = "passwords-delete.json";

/// One saved password. `password` is None if it could not be decrypted.
pub struct Saved {
    pub realm: String,
    pub url: String,
    pub username: String,
    pub password: Option<String>,
}

/// "v10" + nonce + ciphertext + tag -> the password.
fn decrypt_password(key: &[u8], v: &[u8]) -> Option<String> {
    if v.len() < 3 + 12 + 16 || !(v.starts_with(b"v10") || v.starts_with(b"v11")) {
        return None;
    }
    let nonce: [u8; 12] = v[3..15].try_into().ok()?;
    let (ct, tag) = v[15..].split_at(v.len() - 15 - 16);
    let (plain, _) = aes_gcm(key, &nonce, ct, false, Some(tag)).ok()?;
    String::from_utf8(plain).ok()
}

/// Sites + user names deleted in Settings, waiting for the next start.
fn pending_deletes() -> Vec<(String, String)> {
    std::fs::read_to_string(util::data_dir().join(DELETE))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<(String, String)>>(&t).ok())
        .unwrap_or_default()
}

/// Every saved password (minus the ones waiting to be deleted).
pub fn read_store() -> Result<Vec<Saved>, String> {
    let db = profile_dir().join("Default").join("Login Data");
    if !db.exists() {
        return Ok(Vec::new());
    }
    let tmp = util::data_dir().join("login-data-read.tmp");
    std::fs::copy(&db, &tmp).map_err(|e| format!("Could not read Suppa's password store: {e}"))?;
    let result = (|| {
        let key = engine_key()?;
        let conn = Sqlite::load()?.open(&tmp)?;
        let mut st = conn.prepare(
            "SELECT signon_realm, origin_url, username_value, password_value FROM logins \
             WHERE blacklisted_by_user=0 ORDER BY signon_realm, username_value",
        )?;
        let mut out = Vec::new();
        while st.next()? {
            out.push(Saved {
                realm: st.col_text(0),
                url: st.col_text(1),
                username: st.col_text(2),
                password: decrypt_password(&key, &st.col_blob(3)),
            });
        }
        Ok(out)
    })();
    let _ = std::fs::remove_file(&tmp);
    let gone = pending_deletes();
    result.map(|v| v.into_iter().filter(|s| !gone.iter().any(|(r, u)| *r == s.realm && *u == s.username)).collect())
}

/// "https://www.site.com/" -> "site.com"
fn site_name(realm: &str) -> String {
    let host = realm.split("://").nth(1).unwrap_or(realm).split(['/', ':']).next().unwrap_or(realm);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// The list for Settings: sites and user names, never the passwords.
pub fn list_json() -> serde_json::Value {
    match read_store() {
        Ok(items) => serde_json::json!({
            "type": "pw_list",
            "items": items.iter().map(|s| serde_json::json!({
                "site": site_name(&s.realm), "url": s.url, "realm": s.realm,
                "user": s.username, "bad": s.password.is_none(),
            })).collect::<Vec<_>>(),
            "pending": pending_deletes().len(),
        }),
        Err(e) => serde_json::json!({ "type": "pw_list", "items": [], "error": e, "pending": pending_deletes().len() }),
    }
}

fn find(realm: &str, user: &str) -> Option<String> {
    read_store().ok()?.into_iter().find(|s| s.realm == realm && s.username == user)?.password
}

/// One password, when the eye button is clicked.
pub fn reveal(realm: &str, user: &str) -> Option<String> {
    find(realm, user)
}

/// Copies one password to the clipboard (it never goes through the page).
pub fn copy(realm: &str, user: &str) -> bool {
    find(realm, user).is_some_and(|p| util::set_clipboard(&p))
}

/// Deletes one password the next time Suppa starts.
pub fn delete(realm: &str, user: &str) {
    // Also gone from Suppa's vault (and from the other PCs through Dropbox).
    if let Some(o) = origin_of(realm) {
        if crate::vault::with(|v| v.list("logins").iter().any(|x| crate::vault::s(x, "id") == crate::vault::login_id(&o, user) && !crate::vault::deleted(x))) {
            crate::vault::forget_login(&o, user);
        }
    }
    let mut list = pending_deletes();
    if !list.iter().any(|(r, u)| r == realm && u == user) {
        list.push((realm.to_string(), user.to_string()));
    }
    let _ = std::fs::write(util::data_dir().join(DELETE), serde_json::to_string(&list).unwrap_or_default());
}

/// Undo: keeps every password that was waiting to be deleted.
pub fn undo_deletes() {
    let _ = std::fs::remove_file(util::data_dir().join(DELETE));
}

fn delete_logins(db: &Path, list: &[(String, String)]) -> Result<usize, String> {
    if !db.exists() || list.is_empty() {
        return Ok(0);
    }
    let conn = Sqlite::load()?.open(db)?;
    let has = |t: &str| conn.query_text(&format!("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='{t}'")).map(|c| c == "1");
    // Tables that point at a login by its id.
    let mut children = Vec::new();
    for (t, col) in [("logins_edge_extended", "id"), ("insecure_credentials", "parent_id"), ("password_notes", "parent_id")] {
        if has(t)? {
            children.push((t, col));
        }
    }
    conn.exec("BEGIN IMMEDIATE")?;
    let result = (|| {
        let mut n = 0;
        for (realm, user) in list {
            for (t, col) in &children {
                let mut st = conn.prepare(&format!(
                    "DELETE FROM {t} WHERE {col} IN (SELECT id FROM logins WHERE signon_realm=?1 AND username_value=?2)"
                ))?;
                st.text(1, realm)?;
                st.text(2, user)?;
                st.run()?;
            }
            let mut st = conn.prepare("DELETE FROM logins WHERE signon_realm=?1 AND username_value=?2")?;
            st.text(1, realm)?;
            st.text(2, user)?;
            st.run()?;
            drop(st);
            n += conn.changes() as usize;
        }
        Ok(n)
    })();
    match result {
        Ok(n) => {
            conn.exec("COMMIT")?;
            Ok(n)
        }
        Err(e) => {
            let _ = conn.exec("ROLLBACK");
            Err(e)
        }
    }
}

/// Settings -> Passwords -> Export: every saved password to a .csv file
/// (Chrome's format, which Firefox, Chrome, Edge, Brave and Suppa can import).
pub fn export_file() {
    crate::auth::require("Suppa wants to export your saved passwords", |ok| {
        if ok {
            export_confirmed();
        }
    });
}

fn export_confirmed() {
    let hwnd = crate::app::hwnd();
    let list: Vec<Saved> = match read_store() {
        Ok(l) => l.into_iter().filter(|s| s.password.is_some()).collect(),
        Err(e) => {
            util::info(hwnd, &format!("Could not export passwords\n\n{e}"));
            return;
        }
    };
    if list.is_empty() {
        util::info(hwnd, "There are no saved passwords to export.");
        return;
    }
    let n = list.len();
    crate::popup::confirm(
        &format!("Export {n} passwords?"),
        "The file holds every password as plain text: anyone who opens it can read them. Keep it safe and delete it once you have imported it.",
        "Export...",
        "Cancel",
        move || {
            let Some(path) = util::save_dialog("Export passwords", "Suppa Passwords.csv", "Password file (.csv)", "*.csv") else { return };
            let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
            let mut csv = String::from("name,url,username,password,note\r\n");
            for s in &list {
                let url = if s.url.is_empty() { &s.realm } else { &s.url };
                csv += &format!("{},{},{},{},\r\n", q(&site_name(&s.realm)), q(url), q(&s.username), q(s.password.as_deref().unwrap_or("")));
            }
            match std::fs::write(&path, csv) {
                Ok(()) => {
                    util::log(&format!("passwords: {n} exported"));
                    crate::backup::show_notice("✓  Passwords exported", &format!("{n} passwords saved to {}", path.display()));
                }
                Err(e) => util::info(crate::app::hwnd(), &format!("Could not write the file\n\n{e}")),
            }
        },
    );
}

/// Puts logins into the engine's password store at the next start (generated
/// passwords, passwords that came from Dropbox). They are encrypted for the
/// engine right away and added to passwords-import.json, like an import, but
/// without a restart now. `(origin, username, password)`; origin = "https://site.com".
pub fn queue_logins(list: &[(String, String, String)]) -> Result<usize, String> {
    if list.is_empty() {
        return Ok(0);
    }
    let key = engine_key()?;
    let path = util::data_dir().join(PENDING);
    let mut items: Vec<serde_json::Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["items"].as_array().cloned())
        .unwrap_or_default();
    let mut n = 0;
    for (origin, user, pass) in list {
        let Some(origin) = origin_of(origin) else { continue };
        if pass.is_empty() {
            continue;
        }
        // A newer value for the same site and user replaces the waiting one.
        items.retain(|it| !(it["origin"].as_str() == Some(origin.as_str()) && it["username"].as_str() == Some(user.as_str())));
        let enc = encrypt_password(&key, pass)?;
        items.push(serde_json::json!({
            "origin": origin, "action": origin, "username": user,
            "password": base64::engine::general_purpose::STANDARD.encode(enc),
            "created_ms": util::unix_now() * 1000,
        }));
        n += 1;
    }
    std::fs::write(&path, serde_json::json!({ "items": items }).to_string()).map_err(|e| e.to_string())?;
    Ok(n)
}

/// "https://www.site.com/login" -> "https://www.site.com" (None for other schemes).
pub fn origin(url: &str) -> Option<String> {
    origin_of(url)
}
