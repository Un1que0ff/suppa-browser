// SuppaSetup.exe - installs Suppa Browser for the current Windows user.
//
//   SuppaSetup.exe               install (choose the folder, desktop shortcut or not),
//                                or, when Suppa is installed: update or uninstall it
//   SuppaSetup.exe /S            install / update silently (/D=<folder> picks the folder)
//   SuppaSetup.exe --uninstall   remove (used by Windows "Apps": the setup copies
//                                itself next to Suppa.exe for that)
//
// Installs to %LOCALAPPDATA%\Programs\Suppa by default (no admin rights needed),
// adds a Start-menu shortcut (and a desktop one if wanted) and an entry in
// Settings -> Apps. If the
// Microsoft Edge WebView2 Runtime is missing, it offers to install it.
// Its window is drawn in Suppa's own style (see ui.rs).
#![windows_subsystem = "windows"]

mod ui;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
};
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, Interface, PCWSTR, PWSTR, w};

include!(concat!(env!("OUT_DIR"), "/files.rs"));

const VERSION: &str = env!("SUPPA_VERSION");
const APP: &str = "Suppa Browser";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\SuppaBrowser";
const WEBVIEW2_ID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
/// Interface files that are updated on every install. Everything else in
/// scripts\ (your filter rules, your user scripts) is only added if missing.
const UPDATED_FILES: [&str; 12] = [
    "scripts/youtube.js",
    "scripts/ui.html",
    "scripts/ui.css",
    "scripts/ui.js",
    "scripts/menu.html",
    "scripts/settings.html",
    "scripts/history.html",
    "scripts/downloads.html",
    "scripts/cookies.js",
    "scripts/webstore.js",
    "scripts/newtab.html",
    "scripts/README.txt",
];

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let silent = args.iter().any(|a| a.eq_ignore_ascii_case("/S") || a == "--silent");
    if let Some(i) = args.iter().position(|a| a == "--preview") {
        // Pictures of the setup's screens (for checking the look), no installing.
        let out = PathBuf::from(args.get(i + 1).cloned().unwrap_or_else(|| "preview".into()));
        let _ = std::fs::create_dir_all(&out);
        let dir = default_dir();
        ui::preview(
            &out.join("1-install.bmp"),
            &format!("Install Suppa {VERSION}"),
            &format!("Suppa is installed for your Windows account only, no administrator rights needed:\n{}\n\nA Start-menu and a desktop shortcut are added. You can remove it any time in Windows Settings \u{2192} Apps.", dir.display()),
            &["Install", "Cancel"],
            0,
            false,
        );
        ui::preview(&out.join("2-busy.bmp"), "Installing Suppa", "Copying files and adding shortcuts\u{2026}", &[], 0, true);
        ui::preview(&out.join("3-done.bmp"), &format!("Suppa {VERSION} is installed"), "Find it in the Start menu or on your desktop.", &["Start Suppa", "Close"], 0, false);
        return;
    }
    let removing = args.iter().any(|a| a == "--uninstall" || a.eq_ignore_ascii_case("/uninstall"));
    let folder = args.iter().find_map(|a| a.strip_prefix("/D=").or_else(|| a.strip_prefix("--dir="))).map(PathBuf::from);
    let result = if removing {
        // Run from the copy next to Suppa.exe (Settings -> Apps): remove that folder.
        let here = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).filter(|d| d.join("Suppa.exe").exists());
        match here.or_else(installed_dir) {
            Some(dir) => uninstall(&dir, silent),
            None => Err("Suppa is not installed.".into()),
        }
    } else if silent {
        let dir = folder.or_else(installed_dir).unwrap_or_else(default_dir);
        let desktop = !dir.join("Suppa.exe").exists() || has_desktop_shortcut();
        install(&dir, desktop, true)
    } else if let Some(dir) = installed_dir() {
        // Already installed: update it, or uninstall it.
        let cur = installed_version().unwrap_or_else(|| "an older version".into());
        let same = cur == VERSION;
        let update = if same { format!("Reinstall {VERSION}") } else { format!("Update to {VERSION}") };
        let text = format!(
            "Suppa {cur} is installed in\n{}\n\n{} Your tabs, settings, passwords and saved tabs are kept.",
            dir.display(),
            if same { "This is the newest version; you can install it again to repair it." } else { "Update it to the new version, or remove it from this PC." },
        );
        match ui::dialog(&window_title(), &format!("Suppa {cur} is installed"), &text, &[&update, "Uninstall", "Cancel"], 0) {
            Some(0) => install(&dir, has_desktop_shortcut(), false),
            Some(1) => uninstall(&dir, false),
            _ => Ok(()),
        }
    } else {
        // New install: where, and a desktop shortcut or not.
        let start = default_dir();
        let text = "Choose the folder for Suppa. In your user folder (the default) no administrator rights are needed.\nA Start-menu shortcut is added; you can remove Suppa any time in Windows Settings \u{2192} Apps.";
        let (choice, dir, checks) = ui::form(
            &window_title(),
            &format!("Install Suppa {VERSION}"),
            text,
            Some(&start.display().to_string()),
            &[("Add a shortcut to the desktop", true)],
            &["Install", "Cancel"],
            0,
        );
        if choice == Some(0) {
            let dir = dir.map(PathBuf::from).unwrap_or(start);
            install(&dir, checks.first().copied().unwrap_or(true), false)
        } else {
            Ok(())
        }
    };
    if let Err(e) = result {
        if !silent {
            let what = if removing { "Suppa could not be removed" } else { "Suppa could not be installed" };
            ui::dialog(&window_title(), what, &e, &["Close"], 0);
        }
        std::process::exit(1);
    }
}

// ---- small helpers -------------------------------------------------------------------

fn window_title() -> String {
    format!("{APP} {VERSION} setup")
}

/// Yes / no question in the themed window. `yes_first`: Enter means yes.
fn ask(title: &str, text: &str, yes: &str, no: &str, yes_first: bool) -> bool {
    let primary = if yes_first { 0 } else { 1 };
    ui::dialog(&window_title(), title, text, &[yes, no], primary) == Some(0)
}

fn env_path(var: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(var).unwrap_or_default())
}

/// Where a new install goes unless another folder is chosen.
fn default_dir() -> PathBuf {
    env_path("LOCALAPPDATA").join("Programs").join("Suppa")
}

/// Folder of the installed Suppa (from Settings -> Apps), if it is there.
fn installed_dir() -> Option<PathBuf> {
    let reg = reg_get(HKEY_CURRENT_USER, UNINSTALL_KEY, "InstallLocation").map(PathBuf::from);
    reg.into_iter().chain(Some(default_dir())).find(|d| d.join("Suppa.exe").exists())
}

fn installed_version() -> Option<String> {
    reg_get(HKEY_CURRENT_USER, UNINSTALL_KEY, "DisplayVersion")
}

fn desktop_shortcut() -> Option<PathBuf> {
    known_folder(&FOLDERID_Desktop).map(|d| d.join(format!("{APP}.lnk")))
}

fn has_desktop_shortcut() -> bool {
    desktop_shortcut().is_some_and(|p| p.exists())
}

fn reg_get(root: HKEY, path: &str, name: &str) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        RegGetValueW(root, &HSTRING::from(path), &HSTRING::from(name), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)).ok().ok()?;
        let n = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..n])).filter(|s| !s.is_empty())
    }
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// Start-menu shortcut (+ the desktop one).
fn shortcut_paths(desktop: bool) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = known_folder(&FOLDERID_Programs) {
        v.push(p.join(format!("{APP}.lnk")));
    }
    if desktop {
        v.extend(desktop_shortcut());
    }
    v
}

fn reg_set(root: HKEY, path: &str, name: &str, value: &str) {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyW(root, &HSTRING::from(path), &mut key).is_ok() {
            let data: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
            let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
            let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(bytes));
            let _ = RegCloseKey(key);
        }
    }
}

fn reg_set_dword(root: HKEY, path: &str, name: &str, value: u32) {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyW(root, &HSTRING::from(path), &mut key).is_ok() {
            let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes()));
            let _ = RegCloseKey(key);
        }
    }
}

fn reg_has_value(root: HKEY, path: &str, name: &str) -> bool {
    unsafe {
        let mut size = 0u32;
        RegGetValueW(root, &HSTRING::from(path), &HSTRING::from(name), RRF_RT_REG_SZ, None, None, Some(&mut size)).is_ok()
    }
}

/// Is the Microsoft Edge WebView2 Runtime installed (per machine or per user)?
fn webview2_installed() -> bool {
    let p = format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_ID}");
    let u = format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_ID}");
    reg_has_value(HKEY_LOCAL_MACHINE, &p, "pv") || reg_has_value(HKEY_CURRENT_USER, &u, "pv")
}

/// Full path of the program that owns a window.
fn window_exe(h: HWND) -> Option<PathBuf> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(proc);
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// Suppa windows of the installed copy (not other copies, e.g. a portable one).
fn installed_suppa_windows(dir: &Path) -> Vec<HWND> {
    let dir = dir.to_string_lossy().to_lowercase();
    let mut out = Vec::new();
    let mut after: Option<HWND> = None;
    unsafe {
        while let Ok(h) = FindWindowExW(None, after, w!("SuppaBrowserWindow"), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            if window_exe(h).is_some_and(|p| p.to_string_lossy().to_lowercase().starts_with(&dir)) {
                out.push(h);
            }
            after = Some(h);
        }
    }
    out
}

/// Asks the installed Suppa to close (it saves its tabs) and waits.
fn close_suppa(dir: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut asked = false;
    loop {
        let open = installed_suppa_windows(dir);
        if open.is_empty() {
            if asked {
                std::thread::sleep(Duration::from_millis(800)); // let the engine let go of files
            }
            return true;
        }
        for h in open {
            unsafe {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        asked = true;
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

fn write_retry(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut last = String::new();
    for _ in 0..20 {
        match std::fs::write(path, bytes) {
            Ok(()) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(format!("{}: {last}", path.display()))
}

fn make_shortcut(lnk: &Path, target: &Path, workdir: &Path) -> windows::core::Result<()> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(target.as_os_str()))?;
        link.SetWorkingDirectory(&HSTRING::from(workdir.as_os_str()))?;
        link.SetDescription(&HSTRING::from("Tiny, fast, private web browser"))?;
        link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true)
    }
}

fn install_webview2() -> Result<(), String> {
    use windows::Win32::System::Com::Urlmon::URLDownloadToFileW;
    let tmp = std::env::temp_dir().join("MicrosoftEdgeWebview2Setup.exe");
    unsafe {
        URLDownloadToFileW(None, w!("https://go.microsoft.com/fwlink/p/?LinkId=2124703"), &HSTRING::from(tmp.as_os_str()), 0, None)
            .map_err(|e| format!("The WebView2 download failed: {e}"))?;
    }
    let status = std::process::Command::new(&tmp)
        .args(["/silent", "/install"])
        .status()
        .map_err(|e| format!("Could not run the WebView2 installer: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    if !status.success() && !webview2_installed() {
        return Err("The WebView2 Runtime could not be installed.".into());
    }
    Ok(())
}

// ---- install ---------------------------------------------------------------------------

/// Installs (or updates) Suppa in `dir`. `desktop`: add a desktop shortcut
/// (false removes one that is there).
fn install(dir: &Path, desktop: bool, silent: bool) -> Result<(), String> {
    let dir = dir.to_path_buf();
    let exe = dir.join("Suppa.exe");
    let updating = exe.exists();
    let wt = window_title();

    // WebView2 (the engine that shows web pages) - part of up-to-date Windows.
    if !webview2_installed() {
        let go = silent
            || ask(
                "WebView2 is needed",
                "Suppa shows web pages with the Microsoft Edge WebView2 Runtime, which is missing on this PC.\n\nDownload and install it now from Microsoft (about 2 MB)?",
                "Install WebView2",
                "Skip",
                true,
            );
        if go {
            if silent {
                install_webview2()?;
            } else {
                ui::busy(&wt, "Installing WebView2", "Downloading the runtime from Microsoft\u{2026}", install_webview2)?;
            }
        }
    }

    let work = || -> Result<(), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); // for the shortcuts
        }
        if updating && !close_suppa(&dir) {
            return Err("Suppa is still open. Close it and run the setup again.".into());
        }
        write_retry(&exe, SUPPA_EXE)?;
        for (rel, bytes) in FILES {
            let path = dir.join(rel.replace('/', "\\"));
            if UPDATED_FILES.contains(rel) || !path.exists() {
                write_retry(&path, bytes)?;
            }
        }
        // A copy of this setup, used for "Uninstall" in Settings -> Apps.
        let setup_copy = dir.join("SuppaSetup.exe");
        if let Ok(me) = std::env::current_exe() {
            if me.canonicalize().ok() != setup_copy.canonicalize().ok() {
                let _ = std::fs::copy(&me, &setup_copy);
            }
        }
        for lnk in shortcut_paths(desktop) {
            if let Some(p) = lnk.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = make_shortcut(&lnk, &exe, &dir);
        }
        if !desktop {
            if let Some(d) = desktop_shortcut() {
                let _ = std::fs::remove_file(d);
            }
        }
        // Settings -> Apps entry
        let size_kb = (SUPPA_EXE.len() + FILES.iter().map(|(_, b)| b.len()).sum::<usize>()) / 1024;
        let k = UNINSTALL_KEY;
        reg_set(HKEY_CURRENT_USER, k, "DisplayName", APP);
        reg_set(HKEY_CURRENT_USER, k, "DisplayVersion", VERSION);
        reg_set(HKEY_CURRENT_USER, k, "Publisher", "Suppa Browser");
        reg_set(HKEY_CURRENT_USER, k, "DisplayIcon", &format!("{},0", exe.display()));
        reg_set(HKEY_CURRENT_USER, k, "InstallLocation", &dir.display().to_string());
        reg_set(HKEY_CURRENT_USER, k, "UninstallString", &format!("\"{}\" --uninstall", setup_copy.display()));
        reg_set(HKEY_CURRENT_USER, k, "QuietUninstallString", &format!("\"{}\" --uninstall /S", setup_copy.display()));
        reg_set(HKEY_CURRENT_USER, k, "URLInfoAbout", "https://github.com/Un1que0ff/suppa-browser");
        reg_set_dword(HKEY_CURRENT_USER, k, "EstimatedSize", size_kb as u32 + 1);
        reg_set_dword(HKEY_CURRENT_USER, k, "NoModify", 1);
        reg_set_dword(HKEY_CURRENT_USER, k, "NoRepair", 1);
        Ok(())
    };

    if silent {
        work()?;
        if updating {
            let _ = std::process::Command::new(&exe).current_dir(&dir).spawn();
        }
        return Ok(());
    }
    let (t, x) = if updating { ("Updating Suppa", "Closing Suppa and replacing its files\u{2026}") } else { ("Installing Suppa", "Copying files and adding shortcuts\u{2026}") };
    ui::busy(&wt, t, x, work)?;

    let (t, x) = if updating {
        (format!("Suppa is updated to {VERSION}"), "Everything you had open is restored when it starts.".to_string())
    } else {
        (format!("Suppa {VERSION} is installed"), if desktop { "Find it in the Start menu or on your desktop." } else { "Find it in the Start menu." }.to_string())
    };
    if ask(&t, &x, "Start Suppa", "Close", true) {
        ui::hide();
        let _ = std::process::Command::new(&exe).current_dir(&dir).spawn();
    }
    Ok(())
}

// ---- uninstall -------------------------------------------------------------------------

/// Removes Suppa from `dir`: with its data (settings, tabs, passwords...) or keeping it.
fn uninstall(dir: &Path, silent: bool) -> Result<(), String> {
    let dir = dir.to_path_buf();
    let wt = window_title();
    let data = env_path("LOCALAPPDATA").join("SuppaBrowser");
    let delete_data = if silent {
        false
    } else {
        let text = format!(
            "Suppa, its shortcuts and its default-browser registration are removed from\n{}\n\nKeep your settings, saved tabs, open tabs, passwords and extensions if you may install Suppa again later, or remove everything.",
            dir.display()
        );
        match ui::dialog(&wt, "Remove Suppa?", &text, &["Remove, keep settings", "Remove everything", "Cancel"], 0) {
            Some(0) => false,
            Some(1) => true,
            _ => return Ok(()),
        }
    };

    let work = || -> Result<(), String> {
        if !close_suppa(&dir) {
            return Err("Suppa is still open. Close it and try again.".into());
        }
        for lnk in shortcut_paths(true) {
            let _ = std::fs::remove_file(lnk);
        }
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY));
            // "Make Suppa the default browser" registration
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, w!(r"Software\Clients\StartMenuInternet\Suppa"));
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, w!(r"Software\Classes\SuppaHTML"));
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, w!(r"Software\RegisteredApplications"), w!("Suppa"));
        }
        if delete_data {
            let _ = std::fs::remove_dir_all(&data);
        }
        // Remove Suppa's own files now (never anything else that may be in the
        // chosen folder), except this running setup...
        let _ = std::fs::remove_file(dir.join("Suppa.exe"));
        let _ = std::fs::remove_dir_all(dir.join("scripts"));
        for (rel, _) in FILES {
            let _ = std::fs::remove_file(dir.join(rel.replace('/', "\\")));
        }
        // ...which deletes itself and the folder (only if empty) a moment after it exits.
        use std::os::windows::process::CommandExt;
        let setup = dir.join("SuppaSetup.exe");
        let _ = std::process::Command::new("cmd")
            .raw_arg(format!("/c ping 127.0.0.1 -n 3 >nul & del /f /q \"{}\" & rmdir \"{}\"", setup.display(), dir.display()))
            .creation_flags(0x0800_0000) // no console window
            .spawn();
        Ok(())
    };

    if silent {
        return work();
    }
    ui::busy(&wt, "Removing Suppa", "Closing Suppa and removing its files\u{2026}", work)?;
    ui::dialog(&wt, "Suppa is removed", if delete_data { "Your Suppa data was deleted too." } else { "Your Suppa data is kept." }, &["Close"], 0);
    Ok(())
}
