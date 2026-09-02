fn main() {
    // The compiling rustc's version string, baked in for the image
    // fingerprint (docs/impl/image.md § "Fingerprint: regenerate, never
    // migrate"): `HeapObject` is `repr(Rust)`, so an image is valid only for
    // a binary whose compiler agrees with the dumper's layout decisions.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = std::process::Command::new(&rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=ELLE_RUSTC={version}");

    // libgcc is only needed for libffi on Android (__clear_cache symbol).
    if std::env::var("CARGO_FEATURE_FFI").is_ok()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
    {
        println!("cargo:rustc-link-lib=gcc");
    }
}
