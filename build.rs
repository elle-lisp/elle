// audited: 2026-10-21
// Bakes in the rustc version for the fingerprint, names the boot blob the
// library compiles in, and links libgcc where libffi needs it.
// docs/impl/image/format.md
// docs/impl/image/blob.md

fn main() {
    // The compiling rustc's version string, baked in for the image
    // fingerprint: `HeapObject` is `repr(Rust)`, so an image is valid only for
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

    embed_boot_blob();

    // libgcc is only needed for libffi on Android (__clear_cache symbol).
    if std::env::var("CARGO_FEATURE_FFI").is_ok()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
    {
        println!("cargo:rustc-link-lib=gcc");
    }
}

/// Turn on `elle_boot_blob` when `ELLE_BOOT_BLOB` names a file, and hand the
/// library that file's absolute path to include.
///
/// `include_bytes!` reads a relative path against the file that names it, so
/// the path is made absolute here, against the package root cargo runs this
/// script in. Naming a file that does not exist fails the build, because the
/// Makefile only names the blob it has just written.
fn embed_boot_blob() {
    println!("cargo::rustc-check-cfg=cfg(elle_boot_blob)");
    println!("cargo:rerun-if-env-changed=ELLE_BOOT_BLOB");
    let Some(named) = std::env::var_os("ELLE_BOOT_BLOB").filter(|v| !v.is_empty()) else {
        return;
    };
    let path = std::fs::canonicalize(&named)
        .unwrap_or_else(|e| panic!("ELLE_BOOT_BLOB names {named:?}, which does not open: {e}"));
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rustc-cfg=elle_boot_blob");
    println!("cargo:rustc-env=ELLE_BOOT_BLOB_PATH={}", path.display());
}
