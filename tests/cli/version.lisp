(elle/epoch 18)
## audited: 2026-11-22
## Every version string the binary prints derives from the package version in Cargo.toml.
## tests/cli/overview.md
## docs/config.md
##
## The version has one source: `[package] version` in the root Cargo.toml, read
## here from the file. A release bump that misses a hand-written banner fails
## here instead of shipping stale. The language server's copy is pinned beside
## its protocol, in tests/cli/lsp.lisp.

(include-file "../runner/lib/harness.lisp")

(def banner
  (string "Elle v"
          (get (string/split (first (filter
                                      (fn [l]
                                        (string/starts-with? l "version = "))
                                      (string/split (slurp "Cargo.toml") "\n")))
                             "\"") 1)))

(eprintln "scenario: --version prints the banner and nothing else")
(let [r (elle-run ["--version"])]
  (assert (= r:exit 0) (string "--version exits 0; stderr: " r:err))
  (assert (= (string/trim r:out) banner)
          (string "--version prints " banner " alone, got " r:out)))

# The trap: fold --version into the normal flag parse and the binary builds a
# runtime, reads the file, and reports that error first. A tree whose stdlib or
# plugin is broken is when somebody asks what version they hold, so the answer
# cannot depend on a working VM. The counter-factual exits 1 with a "No such
# file" line, and stdout stays empty.
(with-temp-dir dir
  (eprintln "scenario: --version answers before the file argument is read")
  (let [missing (path/join dir "missing.lisp")
        r (elle-run ["--version" missing])]
    (assert (not (file-exists? missing)) "the probe path must not exist")
    (assert (= r:exit 0)
            (string "--version exits 0 whatever follows it; stderr: " r:err))
    (assert (= (string/trim r:out) banner)
            (string "--version answers before the file is read, got " r:out))))

(eprintln "scenario: the help banner derives from the package version")
(let [r (elle-run ["--help"])]
  (assert (string/contains? r:out banner)
          (string "--help names " banner ": " r:out)))

(eprintln "scenario: the REPL banner derives from the package version")
(let [r (elle-feed [] "(exit)\n")]
  (assert (string/contains? r:out banner)
          (string "the REPL greets with " banner ": " r:out)))

(eprintln "all version scenarios passed")
