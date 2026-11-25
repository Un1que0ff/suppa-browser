// SQLite from Windows (winsqlite3.dll): read and write the engine's own files
// (saved passwords, history). No SQLite is linked into Suppa.exe.
use std::path::Path;

use windows::core::w;


pub type Db = *mut std::ffi::c_void;
pub type Stmt = *mut std::ffi::c_void;

pub struct Sqlite {
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
    column_blob: unsafe extern "system" fn(Stmt, i32) -> *const u8,
    column_bytes: unsafe extern "system" fn(Stmt, i32) -> i32,
    changes: unsafe extern "system" fn(Db) -> i32,
    last_insert_rowid: unsafe extern "system" fn(Db) -> i64,
}

pub const SQLITE_OK: i32 = 0;
pub const SQLITE_ROW: i32 = 100;
pub const SQLITE_DONE: i32 = 101;
pub const SQLITE_TRANSIENT: isize = -1;

impl Sqlite {
    pub fn load() -> Result<&'static Sqlite, String> {
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
                column_blob: f!("sqlite3_column_blob"),
                column_bytes: f!("sqlite3_column_bytes"),
                changes: f!("sqlite3_changes"),
                last_insert_rowid: f!("sqlite3_last_insert_rowid"),
            })
        })
        .as_ref()
        .ok_or_else(|| "Windows' SQLite (winsqlite3.dll) is missing.".to_string())
    }

    pub fn open(&'static self, path: &Path) -> Result<Conn, String> {
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

pub struct Conn {
    s: &'static Sqlite,
    db: Db,
}

impl Drop for Conn {
    fn drop(&mut self) {
        unsafe { (self.s.close)(self.db) };
    }
}

impl Conn {
    pub fn err(&self) -> String {
        unsafe {
            let p = (self.s.errmsg)(self.db);
            if p.is_null() {
                return "SQLite error".into();
            }
            std::ffi::CStr::from_ptr(p as *const _).to_string_lossy().into_owned()
        }
    }
    pub fn exec(&self, sql: &str) -> Result<(), String> {
        let s = format!("{sql}\0");
        let rc = unsafe { (self.s.exec)(self.db, s.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
        if rc == SQLITE_OK { Ok(()) } else { Err(self.err()) }
    }
    pub fn prepare(&self, sql: &str) -> Result<St<'_>, String> {
        let s = format!("{sql}\0");
        let mut st: Stmt = std::ptr::null_mut();
        let rc = unsafe { (self.s.prepare_v2)(self.db, s.as_ptr(), -1, &mut st, std::ptr::null_mut()) };
        if rc != SQLITE_OK || st.is_null() {
            return Err(self.err());
        }
        Ok(St { c: self, st })
    }
    /// First column of the first row as text.
    pub fn query_text(&self, sql: &str) -> Result<String, String> {
        let st = self.prepare(sql)?;
        let rc = unsafe { (self.s.step)(st.st) };
        if rc != SQLITE_ROW {
            return Ok(String::new());
        }
        let p = unsafe { (self.s.column_text)(st.st, 0) };
        Ok(if p.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(p as *const _) }.to_string_lossy().into_owned() })
    }
    pub fn changes(&self) -> i32 {
        unsafe { (self.s.changes)(self.db) }
    }
    pub fn last_id(&self) -> i64 {
        unsafe { (self.s.last_insert_rowid)(self.db) }
    }
}

pub struct St<'a> {
    c: &'a Conn,
    st: Stmt,
}

impl Drop for St<'_> {
    fn drop(&mut self) {
        unsafe { (self.c.s.finalize)(self.st) };
    }
}

impl St<'_> {
    pub fn check(&self, rc: i32) -> Result<(), String> {
        if rc == SQLITE_OK { Ok(()) } else { Err(self.c.err()) }
    }
    pub fn text(&mut self, i: i32, v: &str) -> Result<(), String> {
        let rc = unsafe { (self.c.s.bind_text)(self.st, i, v.as_ptr(), v.len() as i32, SQLITE_TRANSIENT) };
        self.check(rc)
    }
    pub fn blob(&mut self, i: i32, v: &[u8]) -> Result<(), String> {
        let rc = unsafe { (self.c.s.bind_blob)(self.st, i, v.as_ptr(), v.len() as i32, SQLITE_TRANSIENT) };
        self.check(rc)
    }
    pub fn int(&mut self, i: i32, v: i64) -> Result<(), String> {
        let rc = unsafe { (self.c.s.bind_int64)(self.st, i, v) };
        self.check(rc)
    }
    /// Next row: true while there is one.
    pub fn next(&mut self) -> Result<bool, String> {
        match unsafe { (self.c.s.step)(self.st) } {
            SQLITE_ROW => Ok(true),
            SQLITE_DONE => Ok(false),
            _ => Err(self.c.err()),
        }
    }
    pub fn col_text(&self, i: i32) -> String {
        let p = unsafe { (self.c.s.column_text)(self.st, i) };
        if p.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(p as *const _) }.to_string_lossy().into_owned() }
    }
    pub fn col_int(&self, i: i32) -> i64 {
        self.col_text(i).parse().unwrap_or(0)
    }
    pub fn col_blob(&self, i: i32) -> Vec<u8> {
        unsafe {
            let p = (self.c.s.column_blob)(self.st, i);
            let n = (self.c.s.column_bytes)(self.st, i);
            if p.is_null() || n <= 0 { Vec::new() } else { std::slice::from_raw_parts(p, n as usize).to_vec() }
        }
    }
    pub fn run(&mut self) -> Result<(), String> {
        let rc = unsafe { (self.c.s.step)(self.st) };
        if rc == SQLITE_DONE || rc == SQLITE_ROW { Ok(()) } else { Err(self.c.err()) }
    }
}
