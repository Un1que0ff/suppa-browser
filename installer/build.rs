// Packs the freshly built Suppa.exe and the scripts folder into SuppaSetup.exe
// (build Suppa first: build.bat does both in the right order).
// Also takes Suppa's version, manifest and icon.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, p));
        }
    }
}

fn main() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = here.parent().unwrap().to_path_buf();
    let exe = root.join("target").join("release").join("Suppa.exe");
    if !exe.exists() {
        panic!("Build Suppa first (target\\release\\Suppa.exe is missing) - run build.bat");
    }
    println!("cargo:rerun-if-changed={}", exe.display());
    println!("cargo:rerun-if-changed={}", root.join("scripts").display());
    println!("cargo:rerun-if-changed={}", root.join("Cargo.toml").display());

    // Suppa's version (from its Cargo.toml)
    let toml = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let version = toml
        .lines()
        .find_map(|l| l.trim().strip_prefix("version = \"").and_then(|v| v.strip_suffix('"')))
        .unwrap_or("0.0.0");
    println!("cargo:rustc-env=SUPPA_VERSION={version}");

    // Embedded files
    let mut files = Vec::new();
    walk(&root.join("scripts"), &root, &mut files);
    let mut src = String::new();
    writeln!(src, "pub static SUPPA_EXE: &[u8] = include_bytes!(r\"{}\");", exe.display()).unwrap();
    writeln!(src, "pub static FILES: &[(&str, &[u8])] = &[").unwrap();
    for (rel, p) in &files {
        println!("cargo:rerun-if-changed={}", p.display());
        writeln!(src, "    (\"{rel}\", include_bytes!(r\"{}\")),", p.display()).unwrap();
    }
    writeln!(src, "];").unwrap();
    std::fs::write(PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("files.rs"), src).unwrap();

    // Same manifest (modern controls, DPI aware) and icon as Suppa.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", root.join("app.manifest").display());
        let res = root.join("target").join("suppa-icon.res");
        if res.exists() {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
    }
}
