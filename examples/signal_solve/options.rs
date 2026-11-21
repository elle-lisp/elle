// audited: 2026-11-21
//! The command line of `signal_solve`: its flags, and the files it analyzes.
//!
//! docs/impl/solver.md

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
