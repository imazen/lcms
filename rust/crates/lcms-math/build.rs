fn main() {
    #[cfg(feature = "math-sys")]
    {
        // Compile the C shim with the SAME flags as the C oracle so the
        // libm calls inside resolve to the real system libm functions
        // (not Rust's compiler_builtins/fdlibm intrinsics, which differ
        // by ~1 ULP).
        cc::Build::new()
            .file("shims/libm_shim.c")
            .opt_level(2)
            .flag_if_supported("-fno-strict-aliasing")
            .compile("libm_shim");
        // unix targets need -lm except musl (libm is inside libc; the
        // system -lm search would pull glibc's libm). MSVC/mingw resolve
        // math in the CRT already.
        let fam = std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
        let envv = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
        if fam == "unix" && envv != "musl" {
            println!("cargo:rustc-link-lib=m");
        }
    }
    // math-rust / math-zen: pure-Rust providers — nothing to build.
}
