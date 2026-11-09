//! Build script: compiles the reference lcms2 C sources into a static library
//! so differential tests can call the real exported C symbols (and the
//! non-static `_cms*` internals) directly.
//!
//! The C reference is CARGO-DRIVEN: on a fresh clone `cargo test` compiles it
//! with the `cc` crate — no autotools, no cmake. lcms2 is ~26 flat C files and
//! needs only its two include dirs. The oracle is keyed on the submodule's git
//! SHA via a stamp file so an unchanged tree never rebuilds.
//!
//! `shims/` holds small C translation units that re-export `static` functions
//! the parity tests need to reach through a public-looking symbol.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const STAMP_FILE: &str = ".lcms-cref-stamp";

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    // crates/lcms-cref -> crates -> rust -> repo root
    let repo_root = manifest
        .ancestors()
        .nth(3)
        .expect("lcms-cref must live at <repo>/rust/crates/lcms-cref")
        .to_path_buf();

    let c_root = repo_root.join("reference/lcms2");
    if !c_root.join("src").exists() {
        panic!(
            "C reference submodule missing at {}.\n\
             Run: git submodule update --init",
            c_root.display(),
        );
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let stamp_path = out_dir.join(STAMP_FILE);

    // Rebuild only when the submodule HEAD (or this script) changes.
    let submodule_head = repo_root.join(".git/modules/reference/lcms2/HEAD");
    let math_mode = env::var("LCMS_CREF_MATH").ok().unwrap_or_else(|| "abi".into());
    let mut stamp_key = format!("lcms-cref-v3:{math_mode}:");
    if submodule_head.exists() {
        println!("cargo:rerun-if-changed={}", submodule_head.display());
        stamp_key.push_str(&fs::read_to_string(&submodule_head).unwrap_or_default());
    }
    // Key the stamp on shim contents too — they change far more often
    // than the pinned submodule
    let shims_key_dir = manifest.join("shims");
    if shims_key_dir.exists() {
        let mut shim_files: Vec<PathBuf> = fs::read_dir(&shims_key_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "c" || x == "h").unwrap_or(false))
            .collect();
        shim_files.sort();
        for f in &shim_files {
            stamp_key.push_str(&fs::read_to_string(f).unwrap_or_default());
        }
    }
    if fs::read_to_string(&stamp_path).unwrap_or_default() == stamp_key
        && out_dir.join("liblcms2cref.a").exists()
    {
        emit_link(&out_dir);
        return;
    }

    let src_dir = c_root.join("src");
    let mut c_files: Vec<PathBuf> = fs::read_dir(&src_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "c").unwrap_or(false))
        .collect();
    c_files.sort();

    let shims_dir = manifest.join("shims");
    if shims_dir.exists() {
        for e in fs::read_dir(&shims_dir).unwrap().filter_map(|e| e.ok()) {
            let p = e.path();
            if p.extension().map(|x| x == "c").unwrap_or(false) {
                c_files.push(p);
            }
        }
    }

    for f in &c_files {
        println!("cargo:rerun-if-changed={}", f.display());
    }

    println!("cargo:rerun-if-env-changed=LCMS_CREF_MATH");
    let math_sys = env::var("LCMS_CREF_MATH").ok().as_deref() == Some("sys");
    if math_sys {
        println!("cargo:rustc-cfg=math_sys");
    }
    let mut b = cc::Build::new();
    b.files(&c_files)
        .include(c_root.join("include"))
        .include(&src_dir)
        .include(&shims_dir)
        .opt_level(2)
        .warnings(false)
        .flag_if_supported("-fno-strict-aliasing") // the half-float union trick
        .out_dir(&out_dir);
    b.compile("lcms2cref");

    fs::write(&stamp_path, &stamp_key).unwrap();
    emit_link(&out_dir);
}

fn emit_link(out_dir: &Path) {
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=lcms2cref");
    // lcms2 calls into libm — except musl, where libm is inside libc
    // and the system -lm search would pull glibc's static libm.
    let fam = std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let envv = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if fam == "unix" && envv != "musl" {
        println!("cargo:rustc-link-lib=m");
    }
}
