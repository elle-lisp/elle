(elle/epoch 18)
## audited: 2026-11-23
## The Unicode generation a run takes from `--unicode=` and the main file's declaration, and the values `elle` refuses.
## tests/cli/overview.md
## docs/strings.md
##
## tests/integration/unicode_generation.rs pins what needs a Rust type: a
## runtime built for one generation, a worker VM's generation, and the
## declaration's compile errors. This file pins what `main` decides.
##
## The divergence vector: U+10EFA is Extend in the Unicode 17 grapheme table
## and unassigned in Unicode 16, so "a" and U+10EFA are one cluster under 17
## and two under 16.

(include-file "../runner/lib/harness.lisp")

(def counts "(println (length \"a\u{10EFA}\"))")

(defn clusters [args]
  "The cluster count `elle ARGS` prints for the vector, once it exits 0."
  (let [r (elle-run [;args "-e" counts])]
    (assert (= r:exit 0) (string "elle " args " exited " r:exit ": " r:err))
    (string/trim r:out)))

(defn refused [args source]
  "Run `elle ARGS -e SOURCE`, require a refusal, and answer its stderr."
  (let [r (elle-run [;args "-e" source])]
    (assert (not (= r:exit 0)) (string "elle " args " ran " source))
    r:err))

# The counter-factual: a flag parsed and never installed would leave every VM
# on the newest tables, and the count would read 1 under both runs.
(eprintln "scenario: --unicode=16.0 selects the Unicode 16 tables")
(assert (= (clusters ["--unicode=16.0"]) "2")
        "U+10EFA does not extend under Unicode 16")

(eprintln "scenario: with no flag, the newest tables")
(assert (= (clusters []) "1") "U+10EFA extends under Unicode 17")

(eprintln "scenario: a flag that conflicts with the declaration stops the run")
(let [err (refused ["--unicode=16"] "(unicode! 17)")]
  (assert (and (string/contains? err "16") (string/contains? err "17"))
          (string "the refusal must name both generations: " err)))

(eprintln "scenario: a generation this build does not vendor stops the run")
(let [err (refused ["--unicode=15"] "1")]
  (assert (string/contains? err "not available in this build")
          (string "the refusal must say the generation is not vendored: " err)))

(eprintln "scenario: a malformed generation stops the run")
(let [err (refused ["--unicode=latest"] "1")]
  (assert (string/contains? err "--unicode")
          (string "the refusal must name the flag: " err)))

# The main file's declaration is read before any VM exists, so it selects the
# generation of the whole run, the first VM included.
(with-temp-dir dir
  (eprintln "scenario: the main file's declaration selects its generation")
  (let [file (write-fixture dir "unicode16.lisp"
                            (string "(unicode! 16)\n"
                                    "(assert (= (unicode!) [16 0 0]) \"the declaration selected 16.0\")\n"
                                    "(assert (= (length \"a\u{10EFA}\") 2) \"U+10EFA does not extend under 16\")\n"))
        r (elle-run [file])]
    (assert (= r:exit 0)
            (string "the file must run under its declared generation: " r:err))))

(eprintln "all unicode-flag scenarios passed")
