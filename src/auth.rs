// "Is it really you?" before saved passwords are shown, copied or exported.
// Windows Hello (PIN, fingerprint, face) when it is set up, otherwise the
// Windows account password. One check is good for a minute, like other browsers.
use std::cell::RefCell;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::core::HSTRING;

use crate::app::{self, *};
use crate::util;

const KEEP: Duration = Duration::from_secs(60);

thread_local! {
    static WAITING: RefCell<Vec<Box<dyn FnOnce(bool)>>> = RefCell::new(Vec::new());
    static UNTIL: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

/// Runs `then(true)` once Windows confirmed the user (right away if that
/// happened less than a minute ago), else `then(false)`.
pub fn require(reason: &str, then: impl FnOnce(bool) + 'static) {
    if UNTIL.with(|u| u.borrow().is_some_and(|t| Instant::now() < t)) {
        then(true);
        return;
    }
    let first = WAITING.with(|w| {
        let mut w = w.borrow_mut();
        w.push(Box::new(then));
        w.len() == 1
    });
    if !first {
        return; // a check is already on screen; this one waits for its answer
    }
    let hwnd = app::hwnd().0 as isize;
    let reason = reason.to_string();
    std::thread::spawn(move || {
        let ok = verify(HWND(hwnd as _), &reason);
        util::post(HWND(hwnd as _), WM_APP_COMMAND, if ok { CMD_AUTH_OK } else { CMD_AUTH_FAIL } as usize, 0);
    });
}

/// The answer came back (on the UI thread).
pub fn done(ok: bool) {
    if ok {
        UNTIL.with(|u| *u.borrow_mut() = Some(Instant::now() + KEEP));
    }
    let list = WAITING.with(|w| std::mem::take(&mut *w.borrow_mut()));
    for f in list {
        f(ok);
    }
}

fn verify(hwnd: HWND, reason: &str) -> bool {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    match hello(hwnd, reason) {
        Some(ok) => ok,
        None => password(hwnd, reason),
    }
}

/// Windows Hello. None = not set up on this PC.
fn hello(hwnd: HWND, reason: &str) -> Option<bool> {
    use windows::Security::Credentials::UI::*;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    let avail = UserConsentVerifier::CheckAvailabilityAsync().ok()?.join().ok()?;
    if avail != UserConsentVerifierAvailability::Available {
        return None;
    }
    let interop = windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>().ok()?;
    let op: windows_future::IAsyncOperation<UserConsentVerificationResult> =
        unsafe { interop.RequestVerificationForWindowAsync(hwnd, &HSTRING::from(reason)) }.ok()?;
    let r = op.join().ok()?;
    util::log(&format!("auth: Windows Hello {r:?}"));
    Some(r == UserConsentVerificationResult::Verified)
}

/// The Windows sign-in box for the current account, checked with LogonUser.
fn password(hwnd: HWND, reason: &str) -> bool {
    use windows::Win32::Security::Credentials::*;
    use windows::Win32::Security::*;
    unsafe {
        let msg = HSTRING::from(reason);
        let caption = HSTRING::from("Suppa");
        let info = CREDUI_INFOW {
            cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
            hwndParent: hwnd,
            pszMessageText: windows::core::PCWSTR(msg.as_ptr()),
            pszCaptionText: windows::core::PCWSTR(caption.as_ptr()),
            ..Default::default()
        };
        let mut pkg = 0u32;
        let mut out: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut out_len = 0u32;
        let rc = CredUIPromptForWindowsCredentialsW(
            Some(&info),
            0,
            &mut pkg,
            None,
            0,
            &mut out,
            &mut out_len,
            None,
            CREDUIWIN_ENUMERATE_CURRENT_USER,
        );
        if rc != 0 || out.is_null() {
            util::log(&format!("auth: sign-in box closed ({rc})"));
            return false;
        }
        let mut user = [0u16; 514];
        let mut domain = [0u16; 338];
        let mut pass = [0u16; 514];
        let (mut nu, mut nd, mut np) = (user.len() as u32, domain.len() as u32, pass.len() as u32);
        let unpacked = CredUnPackAuthenticationBufferW(
            CRED_PACK_PROTECTED_CREDENTIALS,
            out,
            out_len,
            Some(windows::core::PWSTR(user.as_mut_ptr())),
            &mut nu,
            Some(windows::core::PWSTR(domain.as_mut_ptr())),
            Some(&mut nd),
            Some(windows::core::PWSTR(pass.as_mut_ptr())),
            &mut np,
        )
        .is_ok();
        std::ptr::write_bytes(out as *mut u8, 0, out_len as usize);
        windows::Win32::System::Com::CoTaskMemFree(Some(out));
        let mut ok = false;
        if unpacked {
            // "DOMAIN\user" or "user@domain" in the user field.
            let full = String::from_utf16_lossy(&user[..user.iter().position(|&c| c == 0).unwrap_or(0)]);
            let (dom, name) = match full.split_once('\\') {
                Some((d, n)) => (d.to_string(), n.to_string()),
                None => (String::new(), full.clone()),
            };
            let (hname, hdom) = (HSTRING::from(name), HSTRING::from(dom));
            let mut token = HANDLE::default();
            ok = LogonUserW(
                &hname,
                if hdom.is_empty() { None } else { Some(&hdom) }.map(|d| windows::core::PCWSTR(d.as_ptr())).unwrap_or(windows::core::PCWSTR::null()),
                windows::core::PCWSTR(pass.as_ptr()),
                LOGON32_LOGON_INTERACTIVE,
                LOGON32_PROVIDER_DEFAULT,
                &mut token,
            )
            .is_ok();
            if ok {
                let _ = CloseHandle(token);
            }
        }
        pass.fill(0);
        util::log(&format!("auth: account password {}", if ok { "ok" } else { "not accepted" }));
        ok
    }
}
