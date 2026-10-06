(elle/epoch 14)
# audited: 2026-10-06
# The normalized escape dump of ten real suite files matches its snapshot byte for byte.
# docs/impl/escape.md
#
# One true-escape analysis is authoritative, and its consumers (the region
# solver, functionalize, the lowerer's tail-call predicates) read it. A change
# that moves where escape is decided must not change what gets emitted. This
# file snapshots the normalized `escape` dump (compile/dumps → :escape,
# rendered by src/dump/escape.rs) of a set of REAL suite files and pins it
# byte-for-byte, so a change to escape behaviour changes a snapshot and fails
# here.
#
# The dump's last section is `[region_instrs]`, the emitted RC stream, so this
# also fires on any change to WHERE the region solver places a retain or release —
# a wider net than escape alone.
#
# WHAT A DRIFT SAYS. The comparison reads the drift by section
# (tests/modules/snapdiff.lisp) and the failure names it: a drift confined to
# `[region_instrs]`, with the escape verdicts above it byte-identical, is a
# region-placement change and re-blesses; one that moves `[needs_capture]`,
# `[lambda_captures]`, `[return_frontier]` or `[suppressed_decref_regions]` is an
# escape change and wants the argument this file exists to demand. The
# rendered dump is written beside its snapshot as <name>.got (untracked), because
# a drift that does not reproduce leaves the run's two texts as the only account
# of it. A drift also renders a SECOND time: two renders that disagree are an
# unstable renderer rather than a stale snapshot, and the failure says so.
#
# Why these files and not both whole suites: compile/dumps compiles each source
# twice and holds the regions it allocates, which is why the runner captures no
# dumps (docs/test-runner.md). So this pins a bounded set of real files covering
# the escape shapes — scope-exit values, escape-return, HOF-tail, owned-arg,
# reassign, closure, loop-closure, native-result, the general closure file and
# a fiber-boundary file — rather than a curated set of toy reconstructions. The
# second render is on the failure path alone, so a clean run renders once.
#
# Storage: one tests/golden/escape/<name>.snap per file. First run CAPTURES
# (writes the file); later runs COMPARE. To re-bless after an intended change,
# delete the .snap (or the whole dir) and re-run.

(def snapdiff ((import-file "../modules/snapdiff.lisp")))

(def golden-dir "tests/golden/escape")

# The renderer's contract: every dump carries all five, in this order.
(def sections
  ["[needs_capture]" "[lambda_captures]" "[return_frontier]"
   "[suppressed_decref_regions]" "[region_instrs]"])

# [name path] for each pinned real suite file.
(def corpus
  [["scope-exit-values" "tests/lang/scope-exit-values.lisp"]
   ["region-captured-return-move-uaf"
    "tests/impl/region-captured-return-move-uaf.lisp"]
   ["region-hof-tail-return-uaf" "tests/impl/region-hof-tail-return-uaf.lisp"]
   ["region-get-owned-arg-leak" "tests/impl/region-get-owned-arg-leak.lisp"]
   ["region-mutable-reassign-flow"
    "tests/impl/region-mutable-reassign-flow.lisp"]
   ["region-closure-struct" "tests/impl/region-closure-struct.lisp"]
   ["region-loop-local-closure-tail-uaf"
    "tests/impl/region-loop-local-closure-tail-uaf.lisp"]
   ["region-native-result-leak" "tests/impl/region-native-result-leak.lisp"]
   ["closures" "tests/lang/closures.lisp"]
   ["fiber-escape" "tests/lang/fiber-escape.lisp"]])

(defn check-escape-golden [name path]
  (let [src (slurp path)
        render (fn [] (get (compile/dumps src path) :escape))
        escape (render)]
    (assert (string? escape)
            (string "no :escape snapshot for " path
                    " — did it stop compiling?"))
    # Structural sanity (the renderer's contract): all five sections are always
    # present. This guards a FIRST capture from blessing a malformed snapshot —
    # the comparison below only catches drift once a golden exists.
    (each section sections
      (assert (string/contains? escape section)
              (string "escape snapshot for " path " is missing section " section)))
    (let [snap-path (string golden-dir "/" name ".snap")
          got-path (string golden-dir "/" name ".got")]
      (if (path/exists? snap-path)
        (let [r (snapdiff:drift-report escape render (slurp snap-path) sections)]
          (if r
            (begin
              (spit got-path (get r :text))
              (assert false
                      (string (snapdiff:drift-message r) "\n  for " path
                              "\n  rendered dump kept at " got-path
                              "\n  if intended, delete " snap-path
                              " and re-run to re-capture")))
            # A stale .got from an earlier failure would read as evidence about a
            # tree that is now clean.
            (when (path/exists? got-path) (file/delete got-path))))
        (begin
          (spit snap-path escape)
          (println (string "captured " snap-path)))))))

(each entry corpus
  (check-escape-golden (entry 0) (entry 1)))

# A snapshot that no corpus entry names is left over from an entry that was
# renamed or removed. A renamed entry captures a fresh snapshot and passes
# without comparing anything, so an orphan fails the file.
(def pinned (apply set (map (fn [entry] (string (entry 0) ".snap")) corpus)))
(each f (file/ls golden-dir)
  (when (string/ends-with? f ".snap")
    (assert (contains? pinned f)
            (string golden-dir "/" f " is named by no corpus entry: "
                    "rename the entry to match, or delete the snapshot"))))

(println (string "escape golden: " (length corpus) " corpus files pinned"))
