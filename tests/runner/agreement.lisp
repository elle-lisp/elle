(elle/epoch 15)
## audited: 2026-11-18
## Acceptance tests for `elle test`: when two tiers' values agree, and the `diverge` row they record when not.
## tests/runner/overview.md
## docs/test-runner.md
##
## The implementation suite runs this file (`make smoke-impl`). `ELLE` names the
## binary under test; the Makefile exports it.
##
## Every scenario writes its DB, its CAS and its scratch files under one
## directory from `with-temp-dir`, so two runs never share a path and nothing
## survives the run.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  # A fresh session DB of its own for one scenario.
  (defn run-test [name extra]
    (elle-test (path/join dir (string "rt-" name ".db")) extra))

  (defn diverged [r file]
    "The `diverge` rows run R recorded for FILE."
    (filter (fn [row] (= row:status "diverge"))
            (select-results r:db file
                            (string "result.tier AS tier, "
                                    "result.status AS status, "
                                    "result.reason AS reason"))))

  # ── Tiers that disagree record a `diverge` row ─────────────────────────────
  # diverge.lisp returns a value that names its tier. Each tier's row passes,
  # and one more row on tier `*` records the disagreement, renders each
  # tier's value, and fails the run. A build with no JIT has no second value
  # to disagree with. The counter-factual is a runner that compares nothing,
  # which reports this run green.
  (eprintln "scenario: divergence")
  (let [r (run-test "diverge" @[(fixture "diverge.lisp")])
        d (diverged r "diverge.lisp")]
    (if jit?
      (begin
        (assert (not (= r:exit 0))
                (string "diverge: a divergence gates the run; exit " r:exit))
        (assert (= (length d) 1)
                (string "diverge: expected one diverge row, got " (length d)))
        (let [row (get d 0)]
          (assert (= row:tier "*")
                  (string "diverge: the row is on tier *, got " row:tier))
          (assert (string/contains? row:reason "vm=vm-value")
                  (string "diverge: reason=" row:reason))
          (assert (string/contains? row:reason "jit=jit-value")
                  (string "diverge: reason=" row:reason)))
        (let [run (rows r:db "SELECT n_diverge AS d FROM run")]
          (assert (= (get (get run 0) :d) 1)
                  (string "diverge: run.n_diverge is " (get (get run 0) :d)))))
      (begin
        (assert (= r:exit 0)
                (string "diverge: one tier cannot disagree; exit " r:exit
                        " — stderr: " r:err))
        (assert (empty? d) "diverge: one tier recorded a divergence"))))

  # ── Reference values of one type agree ─────────────────────────────────────
  # closure.lisp returns a closure, and nested-closures.lisp holds one in an
  # array, a struct and a list. Each tier builds its own closures, and `=`
  # compares closures by identity. The counter-factual is a runner that
  # compares by `=` alone, which records a divergence for both files.
  (each fx ["closure.lisp" "nested-closures.lisp"]
    (eprintln "scenario: agreement on " fx)
    (let [r (run-test fx @[(fixture fx)])
          res (select-results r:db fx
                              "result.tier AS tier, result.status AS status")]
      (assert (= r:exit 0)
              (string fx ": tiers that agree pass the run; exit " r:exit
                      " — stderr: " r:err))
      (assert (= (row-tiers res) (tier-list form-tiers))
              (string fx ": one row per tier and no diverge row, got "
                      (row-tiers res)))
      (assert (= (get (get (filter (fn [row] (= row:tier "vm")) res) 0) :status)
                 "pass") (string fx ": the vm tier runs the form and passes"))))

  # ── Agreement still finds a disagreement ───────────────────────────────────
  # reference-kinds.lisp returns a closure on the JIT and a parameter
  # elsewhere. nested-diverge.lisp returns an array whose closure agrees and
  # whose keyword names its tier. The counter-factual is a runner that lets
  # every reference value agree, or judges a container by its type alone,
  # which reports both runs green.
  (each spec [["reference-kinds.lisp" "jit=<closure>"]
              ["nested-diverge.lisp" "jit-value"]]
    (let [fx (get spec 0)
          want (get spec 1)
          r (run-test fx @[(fixture fx)])
          d (diverged r fx)]
      (eprintln "scenario: disagreement on " fx)
      (if jit?
        (begin
          (assert (not (= r:exit 0))
                  (string fx ": a divergence gates the run; exit " r:exit))
          (assert (= (length d) 1)
                  (string fx ": expected one diverge row, got " (length d)))
          (assert (string/contains? (get (get d 0) :reason) want)
                  (string fx ": reason=" (get (get d 0) :reason))))
        (begin
          (assert (= r:exit 0)
                  (string fx ": one tier cannot disagree; exit " r:exit
                          " — stderr: " r:err))
          (assert (empty? d) (string fx ": one tier recorded a divergence"))))))

  (eprintln "all agreement scenarios passed"))
