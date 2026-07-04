(elle/epoch 12)
# escape-golden.lisp — the behaviour-preservation oracle for the escape
# consolidation (docs/impl/escape.md).
#
# The consolidation makes one true-escape analysis authoritative and migrates its
# consumers (the region solver, functionalize, the lowerer's tail-call predicates)
# onto it one step at a time. Every step must be BEHAVIOUR-PRESERVING — it
# relocates where escape is decided without changing what gets emitted. This file
# proves that: it
# snapshots the normalized `escape` dump (compile/dumps → :escape, rendered by
# src/dump/escape.rs) of a set of REAL corpus files and pins it byte-for-byte, so
# a migration step that changes escape behaviour changes a snapshot and fails here.
#
# Why these files and not the whole corpus: compile/dumps compiles each source
# twice and leaks regions (docs/test-runner.md § CAS asset capture — it OOMs a
# full make-smoke run, which is why the runner's own dump capture is disabled).
# So this pins a bounded set of real files covering the escape shapes — region
# pins (escape-return, HOF-tail, discarded-tail, owned-arg, reassign, closure,
# loop-closure, native-result), a leak-suite file, a fiber-boundary file, and the
# general closure suite — rather than a curated set of toy reconstructions.
#
# Storage: one tests/golden/escape/<name>.snap per file. First run CAPTURES
# (writes the file); later runs COMPARE. To re-bless after an intended change,
# delete the .snap (or the whole dir) and re-run.

(def golden-dir "tests/golden/escape")

# [name path] for each pinned real corpus file.
(def corpus
  [["region-basic" "tests/elle/region-basic.lisp"]
   ["region-captured-return-move-uaf"
    "tests/elle/region-captured-return-move-uaf.lisp"]
   ["region-hof-tail-return-uaf" "tests/elle/region-hof-tail-return-uaf.lisp"]
   ["region-get-owned-arg-leak" "tests/elle/region-get-owned-arg-leak.lisp"]
   ["region-mutable-reassign-flow"
    "tests/elle/region-mutable-reassign-flow.lisp"]
   ["region-closure-struct" "tests/elle/region-closure-struct.lisp"]
   ["region-loop-local-closure-tail-uaf"
    "tests/elle/region-loop-local-closure-tail-uaf.lisp"]
   ["region-native-result-leak" "tests/elle/region-native-result-leak.lisp"]
   ["closures" "tests/elle/closures.lisp"]
   ["fiber-escape" "tests/elle/fiber-escape.lisp"]])

(defn check-escape-golden [name path]
  (let [src (slurp path)
        escape (get (compile/dumps src path) :escape)]
    (assert (string? escape)
            (string "no :escape snapshot for " path
                    " — did it stop compiling?"))  # Structural sanity (the renderer's contract): all five sections are always
    # present. This guards a FIRST capture from blessing a malformed snapshot —
    # the byte-compare below only catches drift once a golden exists.
    (each section ["[needs_capture]" "[lambda_captures]" "[return_frontier]"
                   "[suppressed_decref_regions]" "[region_instrs]"]
      (assert (string/contains? escape section)
              (string "escape snapshot for " path " is missing section " section)))
    (let [snap-path (string golden-dir "/" name ".snap")]
      (if (path/exists? snap-path)
        (assert (= escape (slurp snap-path))
                (string "escape snapshot drift for " path
                        " — if intended, delete " snap-path
                        " and re-run to re-capture"))
        (begin
          (spit snap-path escape)
          (println (string "captured " snap-path)))))))

(each entry corpus
  (check-escape-golden (entry 0) (entry 1)))

(println (string "escape golden: " (length corpus) " corpus files pinned"))
