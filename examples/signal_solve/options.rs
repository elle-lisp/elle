// audited: 2026-11-21
//! The command line of `signal_solve`: its flags, and the files it analyzes.
//!
//! docs/impl/solver.md

/// What one run of `signal_solve` was asked to do.
pub struct Options {
    /// `--all`: print every row of the report, where it otherwise prints the
    /// lambdas that differ, the first exports and the top levels that raise.
    pub all: bool,
    /// Analyze the files each file's literal imports name; `--no-follow`
    /// turns this off.
    pub follow: bool,
    /// `--expect`: check the `# expect` and `# violates` lines.
    pub expect: bool,
    /// `--prims`: print the signal each fixed binding started with.
    pub prims: bool,
    /// Every argument that is not a flag.
    pub files: Vec<String>,
}

impl Options {
    /// The options `args` names, the program's name left out.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Options {
            all: false,
            follow: true,
            expect: false,
            prims: false,
            files: Vec::new(),
        };
        for arg in args {
            match arg.as_str() {
                "--all" => options.all = true,
                "--no-follow" => options.follow = false,
                "--expect" => options.expect = true,
                "--prims" => options.prims = true,
                _ => options.files.push(arg),
            }
        }
        options
    }
}

#[cfg(test)]
mod tests {
    use super::Options;

    /// The options a command line of `args` names.
    fn parse(args: &[&str]) -> Options {
        Options::parse(args.iter().map(|a| a.to_string()))
    }

    /// The counter-factual read the dump's switch from the environment, so
    /// `--prims` on the command line was taken for a file to analyze.
    #[test]
    fn prims_on_the_command_line_turns_the_dump_on() {
        let options = parse(&["--prims", "a.lisp"]);
        assert!(options.prims, "--prims turns the dump on");
        assert_eq!(options.files, vec!["a.lisp".to_string()]);
    }

    #[test]
    fn each_flag_sets_its_own_option_and_the_rest_are_files() {
        let options = parse(&["a.lisp", "--all", "--no-follow", "--expect", "b.lisp"]);
        assert!(options.all);
        assert!(!options.follow, "--no-follow turns following off");
        assert!(options.expect);
        assert!(!options.prims, "the dump is off unless asked for");
        assert_eq!(
            options.files,
            vec!["a.lisp".to_string(), "b.lisp".to_string()]
        );
    }

    #[test]
    fn a_bare_command_line_follows_imports_and_names_no_file() {
        let options = parse(&[]);
        assert!(options.follow);
        assert!(!options.all && !options.expect && !options.prims);
        assert!(options.files.is_empty());
    }
}
