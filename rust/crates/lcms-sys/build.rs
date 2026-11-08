fn main() {
    // Compile the C shim with the SAME flags as the C oracle so the libm
    // calls inside resolve to the real system libm functions (not Rust's
    // compiler_builtins/fdlibm intrinsics, which differ by ~1 ULP).
    cc::Build::new()
        .file("shims/libm_shim.c")
        .opt_level(2)
        .flag_if_supported("-fno-strict-aliasing")
        .compile("libm_shim");
    // unix targets need -lm; MSVC/mingw resolve math in the CRT already
    if std::env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("unix") {
        println!("cargo:rustc-link-lib=m");
    }
}
