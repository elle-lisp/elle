// audited: 2026-10-21
//! Pins where the caches live: the directory the environment names, and the
//! boot image's directory beneath `--cache=`.
//!
//! docs/config.md
//! docs/impl/image/boot.md

use super::*;

// ── The cache directory (docs/config.md) ──

/// A variable lookup over `set` alone, standing in for the process environment.
fn vars<'a>(set: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        set.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
    }
}

/// The cache directory `set` names, as a path, with `temp-root` as the
/// platform temp directory.
fn cache_dir(set: &[(&str, &str)]) -> Option<std::path::PathBuf> {
    cache_dir_from(vars(set), std::path::Path::new("temp-root")).map(std::path::PathBuf::from)
}

#[test]
fn elle_cache_names_the_cache_directory() {
    assert_eq!(
        cache_dir(&[("ELLE_CACHE", "/var/cache/e"), ("HOME", "/home/u")]),
        Some("/var/cache/e".into())
    );
}

#[test]
fn an_empty_elle_cache_turns_caching_off() {
    assert_eq!(cache_dir(&[("ELLE_CACHE", ""), ("HOME", "/home/u")]), None);
}

#[test]
fn xdg_cache_home_gets_an_elle_directory() {
    assert_eq!(
        cache_dir(&[("XDG_CACHE_HOME", "/xdg"), ("HOME", "/home/u")]),
        Some(std::path::Path::new("/xdg").join("elle"))
    );
}

#[test]
fn home_falls_back_to_the_xdg_default() {
    assert_eq!(
        cache_dir(&[("HOME", "/home/u")]),
        Some(std::path::Path::new("/home/u").join(".cache").join("elle"))
    );
}

/// The counter-factual: the cache followed `TMPDIR` first, so the directory
/// moved with a variable that names scratch space, not a cache.
#[test]
fn the_temp_variables_do_not_choose_the_cache_directory() {
    assert_eq!(
        cache_dir(&[
            ("TMPDIR", "/scratch"),
            ("TMP", "/scratch"),
            ("HOME", "/home/u")
        ]),
        Some(std::path::Path::new("/home/u").join(".cache").join("elle"))
    );
}

/// The counter-factual: with neither `TMPDIR` nor `TMP` set, the cache was
/// off, and every start compiled the standard library with nothing failing.
#[test]
fn with_no_variable_set_the_cache_is_in_the_temp_directory() {
    assert_eq!(
        cache_dir(&[]),
        Some(std::path::Path::new("temp-root").join("elle-cache"))
    );
}

/// An empty variable names no directory, so the lookup reads past it. Only an
/// empty `ELLE_CACHE` turns caching off.
#[test]
fn an_empty_variable_other_than_elle_cache_counts_as_unset() {
    assert_eq!(
        cache_dir(&[("XDG_CACHE_HOME", ""), ("HOME", "/home/u")]),
        Some(std::path::Path::new("/home/u").join(".cache").join("elle"))
    );
    assert_eq!(
        cache_dir(&[("XDG_CACHE_HOME", ""), ("HOME", "")]),
        Some(std::path::Path::new("temp-root").join("elle-cache"))
    );
}

// ── The boot image (docs/impl/image/boot.md) ──

/// The warm cache is on unless a flag turns it off: no flag means the
/// `--cache=` directory's `boot`, for the CLI and an embedding host alike.
#[test]
fn the_warm_cache_is_on_by_default() {
    use crate::image::boot::BootImage;
    assert_eq!(parse_args(&[]).unwrap().boot_image(), BootImage::Process);
    assert_eq!(Config::default().boot_image(), BootImage::Process);
}

/// Each `--boot-image=` value names one policy.
#[test]
fn each_boot_image_value_names_its_policy() {
    use crate::image::boot::BootImage;
    let policy = |flag: &str| parse_args(&[flag]).unwrap().boot_image();
    assert_eq!(policy("--boot-image=off"), BootImage::Off);
    assert_eq!(policy("--boot-image=on"), BootImage::Process);
    assert_eq!(
        policy("--boot-image=/srv/boot"),
        BootImage::Dir(std::path::PathBuf::from("/srv/boot"))
    );
}

/// The default keeps the cache beneath `--cache=`, so an empty `--cache=`
/// turns the warm cache off with the other caches. The counter-factual is a
/// default that found a directory of its own, which a user who switched
/// caching off would still find filling.
#[test]
fn an_empty_cache_flag_turns_the_warm_cache_off() {
    use crate::image::boot::BootImage;
    let off = parse_args(&["--cache="]).unwrap();
    assert_eq!(BootImage::Process.dir_in(&off), None);
    let on = parse_args(&["--cache=/srv/cache"]).unwrap();
    assert_eq!(
        BootImage::Process.dir_in(&on),
        Some(std::path::PathBuf::from("/srv/cache/boot"))
    );
}
