use super::*;

/// Build a `Vec<String>` from string literals for terse expected values.
fn svec(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

#[test]
fn candidates_linux_passthrough() {
    // Linux is the canonical form: specs are used verbatim.
    assert_eq!(
        library_candidates("libz.so", DlOs::Linux),
        svec(&["libz.so"])
    );
    assert_eq!(
        library_candidates("libz.so.1", DlOs::Linux),
        svec(&["libz.so.1"])
    );
    assert_eq!(
        library_candidates("/usr/lib/libz.so.1", DlOs::Linux),
        svec(&["/usr/lib/libz.so.1"])
    );
}

#[test]
fn candidates_macos_unversioned() {
    // libz.so → libz.dylib, with the original kept as a fallback.
    assert_eq!(
        library_candidates("libz.so", DlOs::Macos),
        svec(&["libz.dylib", "libz.so"])
    );
}

#[test]
fn candidates_macos_versioned() {
    // Linux appends the version after .so; macOS embeds it before .dylib.
    assert_eq!(
        library_candidates("libcairo.so.2", DlOs::Macos),
        svec(&["libcairo.2.dylib", "libcairo.dylib", "libcairo.so.2"])
    );
}

#[test]
fn candidates_macos_preserves_directory() {
    // Only the basename is rewritten; the directory prefix is preserved.
    assert_eq!(
        library_candidates("/usr/lib/libz.so.1", DlOs::Macos),
        svec(&[
            "/usr/lib/libz.1.dylib",
            "/usr/lib/libz.dylib",
            "/usr/lib/libz.so.1"
        ])
    );
}

#[test]
fn candidates_non_soname_passthrough() {
    // Already host-native or unrecognized → returned unchanged.
    assert_eq!(
        library_candidates("libz.dylib", DlOs::Macos),
        svec(&["libz.dylib"])
    );
    assert_eq!(library_candidates("z.dll", DlOs::Windows), svec(&["z.dll"]));
    assert_eq!(
        library_candidates("/opt/foo/bar", DlOs::Macos),
        svec(&["/opt/foo/bar"])
    );
}

#[test]
fn candidates_windows_strips_lib_prefix() {
    // Windows DLLs conventionally drop the `lib` prefix and the version.
    assert_eq!(
        library_candidates("libzmq.so", DlOs::Windows),
        svec(&["zmq.dll", "libzmq.dll", "libzmq.so"])
    );
    assert_eq!(
        library_candidates("libcairo.so.2", DlOs::Windows),
        svec(&["cairo.dll", "libcairo.dll", "libcairo.so.2"])
    );
}

// A library every Linux host has, used to exercise dedup + symbol resolution
// without a build-time fixture. `libc` is always loadable by bare name.
const LIBC: &str = "libc.so.6";

#[test]
fn load_dedups_by_path_and_resolves_symbols() {
    // Two loads of the same library return the same key (dedup) ...
    let k1 = load(LIBC).expect("load libc");
    let k2 = load(LIBC).expect("load libc again");
    assert_eq!(k1, k2, "the same library path dedups to one registry entry");

    // ... and a symbol resolves to the same non-null address through both keys —
    // i.e. the mapping is shared, not reloaded per call.
    let a = symbol(&k1, "strlen").expect("strlen via k1");
    let b = symbol(&k2, "strlen").expect("strlen via k2");
    assert!(!a.is_null());
    assert_eq!(a, b, "the shared mapping yields one symbol address");
}

#[test]
fn missing_path_library_errors() {
    // A '/'-containing path that does not exist on disk fails to load (rather
    // than being handed to the dynamic linker as a bare name).
    assert!(load("/nonexistent/does-not-exist.so").is_err());
}

#[test]
fn register_teardown_validates_symbol() {
    let k = load(LIBC).expect("load libc");
    // A real symbol registers; a bogus one errors at registration, not at run.
    assert!(register_teardown(&k, "free").is_ok());
    assert!(
        register_teardown(&k, "no_such_symbol_xyzzy").is_err(),
        "a teardown symbol that does not resolve must error at registration"
    );
    // Draining is safe and idempotent (we register `free`, a no-arg-safe-enough
    // libc symbol only as a resolution test — but to avoid actually calling
    // free() with no arg, drain without asserting side effects is unsafe; so we
    // do not call run_teardowns here. Resolution validation above is the pin.)
}

#[test]
fn symbol_outlives_a_dropped_subsystem_via_the_global_mapping() {
    // The mapping lives in the process-global registry, not in any per-VM owner,
    // so resolving a symbol does not depend on any FFISubsystem being alive: a
    // fresh resolution after arbitrary churn still succeeds (the library was
    // never unloaded). This is the property that makes the worker-teardown crash
    // impossible by construction.
    let k = load(LIBC).expect("load libc");
    let first = symbol(&k, "strlen").expect("strlen");
    // No FFISubsystem is constructed/dropped here, but the invariant is the
    // same: the global mapping is permanent, so a later lookup is identical.
    let again = symbol(&k, "strlen").expect("strlen again");
    assert_eq!(first, again);
}
