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
use windows::core::w;

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
fn aes_gcm(key: &[u8], nonce: &[u8; 12], data: &[u8], encrypt: bool, tag_in: Option<&[u8]>) -> Result<(Vec<u8>, [u8; 16]), String> {
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

/// At startup, before the engine opens its files: put scheduled passwords in.
pub fn apply_pending() {
    let pending = util::data_dir().join(PENDING);
    let Ok(text) = std::fs::read_to_string(&pending) else { return };
    let db = profile_dir().join("Default").join("Login Data");
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

// ---- SQLite from Windows (winsqlite3.dll) ----------------------------------------------

type Db = *mut std::ffi::c_void;
type Stmt = *mut std::ffi::c_void;

struct Sqlite {
    open_v2: unsafe extern "system" fn(*const u8, *mut Db, i32, *const u8) -> i32,
    close: unsafe extern "system" fn(Db) -> i32,
    busy_timeout: unsafe extern "system" fn(Db, i32) -> i32,
    exec: unsafe extern "system" fn(Db, *const u8, *const std::ffi::c_void, *const std::ffi::c_void, *mut *mut u8) -> i32,
    errmsg: unsafe extern "system" fn(Db) -> *const u8,
    prepare_v2: unsafe extern "system" fn(Db, *const u8, i32, *mut Stmt, *mut *const u8) -> i32,
    bind_text: unsafe extern "system" fn(Stmt, i32, *const u8, i32, isize) -> i32,
    bind_blob: unsafe extern "system" fn(Stmt, i32, *const u8, i32, isize) -> i32,
    bind_int64: unsafe extern "system" fn(Stmt, i32, i64) -> i32,
    step: unsafe extern "system" fn(Stmt) -> i32,
    finalize: unsafe extern "system" fn(Stmt) -> i32,
    column_text: unsafe extern "system" fn(Stmt, i32) -> *const u8,
    changes: unsafe extern "system" fn(Db) -> i32,
    last_insert_rowid: unsafe extern "system" fn(Db) -> i64,
}

const SQLITE_OK: i32 = 0;
const SQLITE_ROW: i32 = 100;
const SQLITE_DONE: i32 = 101;
const SQLITE_TRANSIENT: isize = -1;

impl Sqlite {
    fn load() -> Result<&'static Sqlite, String> {
        use std::sync::OnceLock;
        static S: OnceLock<Option<Sqlite>> = OnceLock::new();
        S.get_or_init(|| unsafe {
            use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
            let lib = LoadLibraryW(w!("winsqlite3.dll")).ok()?;
            macro_rules! f {
                ($n:literal) => {
                    std::mem::transmute(GetProcAddress(lib, windows::core::PCSTR(concat!($n, "\0").as_ptr()))?)
                };
            }
            Some(Sqlite {
                open_v2: f!("sqlite3_open_v2"),
                close: f!("sqlite3_close"),
                busy_timeout: f!("sqlite3_busy_timeout"),
                exec: f!("sqlite3_exec"),
                errmsg: f!("sqlite3_errmsg"),
                prepare_v2: f!("sqlite3_prepare_v2"),
                bind_text: f!("sqlite3_bind_text"),
                bind_blob: f!("sqlite3_bind_blob"),
                bind_int64: f!("sqlite3_bind_int64"),
                step: f!("sqlite3_step"),
                finalize: f!("sqlite3_finalize"),
                column_text: f!("sqlite3_column_text"),
                changes: f!("sqlite3_changes"),
                last_insert_rowid: f!("sqlite3_last_insert_rowid"),
            })
        })
        .as_ref()
        .ok_or_else(|| "Windows' SQLite (winsqlite3.dll) is missing.".to_string())
    }

    fn open(&'static self, path: &Path) -> Result<Conn, String> {
        let p = format!("{}\0", path.display());
        let mut db: Db = std::ptr::null_mut();
        // SQLITE_OPEN_READWRITE
        let rc = unsafe { (self.open_v2)(p.as_ptr(), &mut db, 0x2, std::ptr::null()) };
        let conn = Conn { s: self, db };
        if rc != SQLITE_OK {
            return Err(conn.err());
        }
        // The previous Suppa's engine may hold the file for a moment.
        unsafe { (self.busy_timeout)(db, 8000) };
        Ok(conn)
    }
}

struct Conn {
    s: &'static Sqlite,
    db: Db,
}

impl Drop for Conn {
    fn drop(&mut self) {
        unsafe { (self.s.close)(self.db) };
    }
}

impl Conn {
    fn err(&self) -> String {
        unsafe {
            let p = (self.s.errmsg)(self.db);
            if p.is_null() {
                return "SQLite error".into();
            }
            std::ffi::CStr::from_ptr(p as *const _).to_string_lossy().into_owned()
        }
    }
    fn exec(&self, sql: &str) -> Result<(), String> {
        let s = format!("{sql}\0");
        let rc = unsafe { (self.s.exec)(self.db, s.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
        if rc == SQLITE_OK { Ok(()) } else { Err(self.err()) }
    }
    fn prepare(&self, sql: &str) -> Result<St<'_>, String> {
        let s = format!("{sql}\0");
        let mut st: Stmt = std::ptr::null_mut();
        let rc = unsafe { (self.s.prepare_v2)(self.db, s.as_ptr(), -1, &mut st, std::ptr::null_mut()) };
        if rc != SQLITE_OK || st.is_null() {
            return Err(self.err());
        }
        Ok(St { c: self, st })
    }
    /// First column of the first row as text.
    fn query_text(&self, sql: &str) -> Result<String, String> {
        let st = self.prepare(sql)?;
        let rc = unsafe { (self.s.step)(st.st) };
        if rc != SQLITE_ROW {
            return Ok(String::new());
        }
        let p = unsafe { (self.s.column_text)(st.st, 0) };
        Ok(if p.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(p as *const _) }.to_string_lossy().into_owned() })
    }
    fn changes(&self) -> i32 {
        unsafe { (self.s.changes)(self.db) }
    }
    fn last_id(&self) -> i64 {
        unsafe { (self.s.last_insert_rowid)(self.db) }
    }
}

struct St<'a> {
    c: &'a Conn,
    st: Stmt,
}

impl Drop for St<'_> {
    fn drop(&mut self) {
        unsafe { (self.c.s.finalize)(self.st) };
    }
}

impl St<'_> {
    fn check(&self, rc: i32) -> Result<(), String> {
        if rc == SQLITE_OK { Ok(()) } else { Err(self.c.err()) }
    }
    fn text(&mut self, i: i32, v: &str) -> Result<(), String> {
        let rc = unsafe { (self.c.s.bind_text)(self.st, i, v.as_ptr(), v.len() as i32, SQLITE_TRANSIENT) };
        self.check(rc)
    }
    fn blob(&mut self, i: i32, v: &[u8]) -> Result<(), String> {
        let rc = unsafe { (self.c.s.bind_blob)(self.st, i, v.as_ptr(), v.len() as i32, SQLITE_TRANSIENT) };
        self.check(rc)
    }
    fn int(&mut self, i: i32, v: i64) -> Result<(), String> {
        let rc = unsafe { (self.c.s.bind_int64)(self.st, i, v) };
        self.check(rc)
    }
    fn run(&mut self) -> Result<(), String> {
        let rc = unsafe { (self.c.s.step)(self.st) };
        if rc == SQLITE_DONE || rc == SQLITE_ROW { Ok(()) } else { Err(self.c.err()) }
    }
}
