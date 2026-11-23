(elle/epoch 18)
## audited: 2026-11-22
## The commit hook formats the staged content of a Rust file as it does a Lisp file's, in a plain commit and a partial one.
## tests/cli/overview.md
## CONTRIBUTING.md
##
## On a staged `.rs` file the hook runs rustfmt, then clippy and rustdoc over
## the workspace, so each scratch repository here is a crate. Its first commit
## is formatted, so clippy and rustdoc have nothing to object to.

(include-file "../runner/lib/harness.lisp")
(include-file "lib/pre-commit.lisp")

(defn crate-repo [root name]
  (let [r (scratch-repo root name)]
    (repo-seed r "Cargo.toml"
               (string "[package]\nname = \"scratch\"\nversion = \"0.1.0\"\n"
                       "edition = \"2021\"\n\n[workspace]\n"))
    (repo-seed r "src/lib.rs"
               "//! Scratch.\n\n/// One.\npub fn one() -> u32 {\n    1\n}\n")
    r))

# A crate root that rustfmt reformats.
(def untidy-rs "//! Scratch.\n\n/// Two.\npub fn two()->u32{ 2 }\n")

(with-temp-dir dir
  (eprintln "scenario: a partial commit of an unformatted Rust file is refused")
  (let [r (crate-repo dir "partial-rust")]
    (repo-write r "src/lib.rs" untidy-rs)
    (assert (not (= 0 (get (repo-commit r ["--" "src/lib.rs"]) :exit)))
            "the hook must refuse a partial commit it reformatted")
    (assert (not (= (repo-read r "src/lib.rs") untidy-rs))
            "the hook formatted the file on disk")
    (at-head r)
    (let [out (repo-commit r ["--" "src/lib.rs"])]
      (assert (= out:exit 0)
              (string "the second attempt commits the formatted file: "
                      (transcript out))))
    (assert (= (repo-read r "src/lib.rs") (repo-head r "src/lib.rs"))
            "HEAD holds the disk's file")
    (at-head r))

  (eprintln "scenario: a plain commit leaves an unstaged Rust change unstaged")
  (let [r (crate-repo dir "unstaged-rust")]
    (repo-write r "src/lib.rs" untidy-rs)
    (must r ["add" "src/lib.rs"])
    (repo-write r "src/lib.rs"
                (string untidy-rs "\n/// Three.\npub fn three()->u32{ 3 }\n"))
    (let [out (repo-commit r [])]
      (assert (= out:exit 0) (string "the commit failed: " (transcript out))))
    (let [head (repo-head r "src/lib.rs")]
      (assert (and (string/contains? head "pub fn two() -> u32 {")
                   (not (string/contains? head "three")))
              (string "the commit takes the staged content, formatted, and nothing else: "
                      head)))
    (at-head r)
    (assert (string/contains? (repo-read r "src/lib.rs")
                              "pub fn three() -> u32 {")
            (string "the working copy keeps its unstaged change, formatted: "
                    (repo-read r "src/lib.rs")))))

(eprintln "all pre-commit-rust scenarios passed")
