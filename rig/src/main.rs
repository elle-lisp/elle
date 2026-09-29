// audited: 2026-09-29
//! The rig's entry point: answer a subcommand, or read the sidecar and the profile and run the program through `elle::program`.
//!
//! rig/overview.md

mod settings;

use settings::Settings;
use std::path::Path;

/// The rig's own flags, split from the rest of the command line.
struct RigArgs {
    /// `--profile PATH`: one configuration applied over the file's sidecar.
    profile: Option<String>,
    /// `--print-config`: print the configuration and run nothing.
    print_config: bool,
    /// Every other argument, elle's flags first and the program after them.
    elle_args: Vec<String>,
}

impl RigArgs {
    /// Take `--profile` and `--print-config` from the flags before the program
    /// name. After it, every argument belongs to the program, as under `elle`.
    fn split(args: Vec<String>) -> Result<RigArgs, String> {
        let mut rig = RigArgs {
            profile: None,
            print_config: false,
            elle_args: Vec::new(),
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg == "--" || arg == "-" || !arg.starts_with('-') {
                rig.elle_args.push(arg);
                rig.elle_args.extend(args);
                break;
            }
            match arg.as_str() {
                "--profile" => {
                    rig.profile = Some(args.next().ok_or("--profile requires a path")?);
                }
                "--print-config" => rig.print_config = true,
                "-e" | "--eval" => {
                    rig.elle_args.push(arg);
                    rig.elle_args.extend(args.next());
                }
                _ => rig.elle_args.push(arg),
            }
        }
        Ok(rig)
    }
}

/// The program file among what `Config::parse` left over: the first argument
/// that is not an `-e` expression, unless that is stdin or the separator.
fn program_file(remaining: &[String]) -> Option<&str> {
    remaining
        .iter()
        .find(|arg| !arg.starts_with("--eval:"))
        .map(String::as_str)
        .filter(|arg| *arg != "-" && *arg != "--")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // elle's subcommands answer here as they do under `elle`, before any rig
    // flag is read, so a program that runs its own executable runs unchanged.
    if let Some(code) = elle::program::subcommand(&args) {
        std::process::exit(code);
    }
    let rig = RigArgs::split(args).unwrap_or_else(|e| refuse(&e));
    let (mut config, remaining) =
        elle::config::Config::parse(&rig.elle_args).unwrap_or_else(|e| refuse(&e));
    if config.help {
        println!("Usage: elle-rig [--profile PATH] [--print-config] [elle flags] [file] [args...]");
        println!("See rig/overview.md; every flag `elle --help` lists applies here too.");
        return;
    }
    if config.version {
        println!("{}", elle::BANNER);
        return;
    }

    // The sidecar first, then the profile over it: a profile's tier replaces
    // the sidecar's and its trace keywords join them (rig/overview.md).
    let mut settings = Settings::from_config(&config);
    if let Some(program) = program_file(&remaining) {
        let sidecar = Path::new(program).with_extension("toml");
        if sidecar.is_file() {
            settings.apply_file(&sidecar).unwrap_or_else(|e| refuse(&e));
        }
    }
    if let Some(profile) = &rig.profile {
        settings
            .apply_file(Path::new(profile))
            .unwrap_or_else(|e| refuse(&e));
    }

    if rig.print_config {
        print!("{}", settings.render());
        return;
    }
    settings.install(&mut config);
    std::process::exit(elle::program::run(config, remaining));
}

/// Refuse the run: name why on stderr, run nothing, and exit 2.
fn refuse(why: &str) -> ! {
    eprintln!("elle-rig: {why}");
    std::process::exit(2);
}
