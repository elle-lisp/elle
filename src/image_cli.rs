// audited: 2026-09-21
//! `elle image dump-boot FILE`: boot from source and write the result as a
//! boot image.
//!
//! docs/impl/image/boot.md

use elle::compiler::stdlib_cache::StdlibCache;
use elle::runtime::Runtime;

/// Run the `image` subcommand. Answers the process exit code.
pub(super) fn run_image(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("dump-boot") => match args.get(1) {
            Some(path) => dump_boot(std::path::Path::new(path)),
            None => usage("dump-boot needs the file to write"),
        },
        Some(other) => usage(&format!("unknown image command '{other}'")),
        None => usage("image needs a command"),
    }
}

fn usage(problem: &str) -> i32 {
    eprintln!("elle image: {problem}");
    eprintln!("Usage: elle image dump-boot FILE");
    1
}

/// Boot from source and dump the boot state to `path`.
///
/// The boot is always from source, so the artifact is the one those three
/// sources produce — an image built by hydrating an image is two hops from
/// anything a reader could rebuild and compare. The stdlib disk cache is off
/// for the same reason, and for a second one: a cache hit rebuilds the
/// library's closures through the send codec, whose capture cells record no
/// binding, and the dump refuses one of those (docs/impl/image/boot.md).
fn dump_boot(path: &std::path::Path) -> i32 {
    let (config, _rest) = match elle::config::Config::parse(&[]) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("elle image: {e}");
            return 1;
        }
    };
    elle::config::init(config);
    elle::io::init_process_signals();

    let mut rt = Runtime::with_stdlib_cache(StdlibCache::Off);
    let code = match rt.dump_boot_image(path) {
        Ok(()) => {
            let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            println!("{} ({} bytes)", path.display(), bytes);
            0
        }
        Err(e) => {
            eprintln!("elle image: {e}");
            1
        }
    };
    let _ = rt.teardown();
    code
}
