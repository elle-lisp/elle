// audited: 2026-11-27
//! Pins `--jit=`: the policy each value sets in a build whose tier is the JIT,
//! the values it refuses, and the builds that do not know the flag.
//!
//! docs/config.md

use super::*;

/// The flag in a build whose tier is the JIT.
#[cfg(all(feature = "jit", not(feature = "mlir"), not(feature = "wasm")))]
mod tier {
    use super::*;

    /// `off` turns the JIT off, `0` makes it eager, and a count N compiles a
    /// function once it has been called N times. The counter-factual is the
    /// old reading of the flag, where `0` meant off and N compiled after N-1
    /// calls: a user who asked for eager would run the interpreter alone.
    #[test]
    fn each_value_sets_its_policy() {
        for (flag, want) in [
            ("--jit=off", JitPolicy::Off),
            ("--jit=0", JitPolicy::Eager),
            ("--jit=1", JitPolicy::Adaptive { threshold: 1 }),
            ("--jit=3", JitPolicy::Adaptive { threshold: 3 }),
            ("--jit=10", JitPolicy::Adaptive { threshold: 10 }),
        ] {
            assert_eq!(parse_args(&[flag]).unwrap().jit, want, "{flag}");
        }
    }

    /// `--jit=10` names the policy a JIT build starts with, so the flag and no
    /// flag give one run.
    #[test]
    fn ten_is_the_default() {
        assert_eq!(
            parse_args(&["--jit=10"]).unwrap().jit,
            Config::default().jit
        );
    }

    /// The last `--jit=` wins, as for every other flag.
    #[test]
    fn the_last_flag_wins() {
        assert_eq!(
            parse_args(&["--jit=off", "--jit=0"]).unwrap().jit,
            JitPolicy::Eager
        );
    }

    /// A word the flag does not know is refused with the flag and the value
    /// named. The counter-factual: a reader that took anything unparsed as the
    /// default would run `--jit=eager` adaptive, and the run would pass with
    /// the policy it asked for never in force.
    #[test]
    fn the_flag_refuses_any_other_value_by_name() {
        for value in ["eager", "adaptive", "", "-1", "ten", "OFF", "1.5"] {
            let flag = format!("--jit={value}");
            let err = parse_args(&[flag.as_str()]).unwrap_err();
            assert!(
                err.contains("--jit") && err.contains(&format!("'{value}'")),
                "refusing {flag} must name the flag and the value, got {err:?}"
            );
        }
    }
}

/// A build whose tier is not the JIT does not know the flag, as a build
/// without the `wasm` feature does not know `--wasm=`. The counter-factual: an
/// `mlir` build that read `--jit=0` would run two optimizing tiers.
#[cfg(not(all(feature = "jit", not(feature = "mlir"), not(feature = "wasm"))))]
#[test]
fn a_build_without_the_jit_tier_refuses_the_flag() {
    for flag in ["--jit=off", "--jit=0", "--jit=10"] {
        let err = parse_args(&[flag, "prog.lisp"]).unwrap_err();
        assert!(
            err.contains("unknown option") && err.contains(flag),
            "{flag} must be refused as an unknown option, got {err:?}"
        );
    }
}
