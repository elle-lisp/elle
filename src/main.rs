// audited: 2026-09-29
//! The `elle` binary: dispatch a subcommand, or parse elle's flags and hand the
//! program to `elle::program`.
//!
//! docs/config.md

use std::env;

mod help;
use help::print_help;

fn main() {
    // dlopen'd C++ plugins (e.g. oxigraph) allocate from glibc's static TLS
    // block at load time. glibc 2.39+ grows that reservation on demand, so no
    // up-front reservation is needed here. If plugin loading ever fails with
    // "cannot allocate memory in static TLS block", set
    // GLIBC_TUNABLES=glibc.rtld.optional_static_tls=65536 before launching elle.

    let args: Vec<String> = env::args().collect();

    // A subcommand answers before elle's own flags are read. The rig answers the
    // same ones through the same function (rig/overview.md).
    if let Some(code) = elle::program::subcommand(&args[1..]) {
        std::process::exit(code);
    }

    let (config, remaining_args) = elle::config::Config::parse(&args[1..]).unwrap_or_else(|e| {
        eprintln!("elle: {}", e);
        std::process::exit(1);
    });

    // --help and --version answer before VM init, so they still answer in a
    // tree whose stdlib or plugin is broken — which is when somebody asks. They
    // are elle's only before the program name, where `Config::parse` stops, so a
    // script carries a --help or a --version of its own.
    if config.help {
        print_help();
        return;
    }
    if config.version {
        println!("{}", elle::BANNER);
        return;
    }

    std::process::exit(elle::program::run(config, remaining_args));
}
