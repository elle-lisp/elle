(elle/epoch 13)
## audited: 2026-09-29
## Acceptance tests for `elle test`, per tier: each scenario drives the runner
## as a subprocess and asserts on the rows it records for each tier and each
## JIT policy.
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

               # ── Scenario 1: a form runs on every tier the build carries ───────────────
               # One row per tier, and the run names the tiers it probed. The
               # counter-factual is a runner that runs each file once, on the build's
               # runtime, and records one row.
               (eprintln "scenario: pass")
               (let [r (run-test "pass" @[(fixture "pass.lisp")])]
                 (assert (= r:exit 0)
                         (string "pass: an all-green run exits 0, got " r:exit
                                 " — stderr: " r:err))
                 (let [res (select-results r:db "pass.lisp"
                       "result.tier AS tier, result.status AS status")]
                   (assert (= (row-tiers res) (tier-list form-tiers))
                           (string "pass: one row per tier "
                                   (tier-list form-tiers) ", got "
                                   (row-tiers res)))
                   (each row res
                     (assert (= row:status "pass")
                             (string "pass: tier " row:tier " status "
                                     row:status))))
                 (let [run (rows r:db "SELECT tiers FROM run")]
                   (assert (= (tier-list (string/split (get (get run 0) :tiers)
                              ",")) (tier-list form-tiers))
                           (string "pass: run.tiers is "
                                   (get (get run 0) :tiers)))))

               # ── Scenario 2: a failing form — status, signal, and assert-macro payload ─
               (eprintln "scenario: fail + assert payload")
               (let [r (run-test "fail" @[(fixture "fail.lisp")])]
                 (assert (not (= r:exit 0))
                         "fail: any failure makes the gate exit nonzero")
                 (let [res (select-results r:db "fail.lisp"
                       (string "result.status AS status, "
                               "result.signal AS signal, "
                               "result.syntax AS syntax, "
                               "result.expected AS expected, "
                               "result.actual AS actual, "
                               "form.label AS label, " "result.tier AS tier"))]
                   (assert (= (row-tiers res) (tier-list form-tiers))
                           (string "fail: one row per tier, got "
                                   (row-tiers res)))
                   (each row res
                     (assert (= row:status "fail")
                             (string "fail: tier " row:tier " status="
                                     row:status)))
                   (let [row (get (filter (fn [row] (= row:tier "vm")) res) 0)]
                     (assert (= row:status "fail")
                             (string "fail: status=" row:status))
                     (assert (= row:signal ":failed-assertion")
                             (string "fail: signal=" row:signal))
                     (assert (= row:label "wrong sum")
                             (string "fail: label=" row:label))
                     # The macro captured the predicate, unevaluated, as data.
                     (assert (= row:syntax "(= (+ 1 1) 3)")
                             (string "fail: syntax=" row:syntax))
                     # A comparison (= LHS RHS) records actual = LHS and expected = RHS.
                     (assert (= row:actual "2")
                             (string "fail: actual=" row:actual))
                     (assert (= row:expected "3")
                             (string "fail: expected=" row:expected)))))

               # ── Scenario 3: a gated form is a skip that carries its reason ─────────────
               # The gate's condition is a fact about the machine, read at run time. A
               # language test never gates on a tier, so neither does this fixture.
               (eprintln "scenario: gate! skip")
               (let [r (run-test "gated" @[(fixture "gated.lisp")])]
                 (assert (= r:exit 0)
                         (string "gated: a skip is not a failure; exit " r:exit
                                 " — stderr: " r:err))
                 (let [res (select-results r:db "gated.lisp"
                       (string "result.tier AS tier, "
                               "result.status AS status, "
                               "result.reason AS reason"))]
                   (assert (= (row-tiers res) (tier-list form-tiers))
                           (string "gated: one row per tier, got "
                                   (row-tiers res)))
                   (each row res
                     (assert (= row:status "skip")
                             (string "gated: tier " row:tier " status "
                                     row:status))
                     (assert (= row:reason "needs a widget")
                             (string "gated: tier " row:tier " reason="
                                     row:reason)))))

               # ── Scenario 3b: tiers that disagree record a `diverge` row ────────────────
               # diverge.lisp returns a value that names its tier. Each tier's row passes,
               # and one more row on tier `*` records the disagreement, renders each
               # tier's value, and fails the run. A build with no JIT has no second value
               # to disagree with. The counter-factual is a runner that compares nothing,
               # which reports this run green.
               (eprintln "scenario: divergence")
               (let [r (run-test "diverge" @[(fixture "diverge.lisp")])
                     res (select-results r:db "diverge.lisp"
                     (string "result.tier AS tier, " "result.status AS status, "
                             "result.reason AS reason"))
                     diverged (filter (fn [row] (= row:status "diverge")) res)]
                 (if jit?
                   (begin
                     (assert (not (= r:exit 0))
                             (string "diverge: a divergence gates the run; exit "
                                     r:exit))
                     (assert (= (length diverged) 1)
                             (string "diverge: expected one diverge row, got "
                                     (length diverged)))
                     (let [row (get diverged 0)]
                       (assert (= row:tier "*")
                               (string "diverge: the row is on tier *, got "
                                       row:tier))
                       (assert (string/contains? row:reason "vm=vm-value")
                               (string "diverge: reason=" row:reason))
                       (assert (string/contains? row:reason "jit=jit-value")
                               (string "diverge: reason=" row:reason)))
                     (let [run (rows r:db "SELECT n_diverge AS d FROM run")]
                       (assert (= (get (get run 0) :d) 1)
                               (string "diverge: run.n_diverge is "
                                       (get (get run 0) :d)))))
                   (begin
                     (assert (= r:exit 0)
                             (string "diverge: one tier cannot disagree; exit "
                                     r:exit " — stderr: " r:err))
                     (assert (empty? diverged)
                             "diverge: one tier recorded a divergence"))))

               # ── Scenario 6: a multi-form file is ONE whole-file thunk, run in order ───
               # multi.lisp is the counter-factual for per-form slicing, which hoisted
               # `def`/`var` ahead of the bare-expression forms so `(def snap (get cell 0))`
               # ran before the `(put cell 0 …)` write and read pre-write garbage. As one
               # thunk the write precedes the read, and the file is one form with a row per
               # JIT policy.
               (eprintln "scenario: multi-form whole-file (ordered)")
               (let [r (run-test "multi" @[(fixture "multi.lisp")])]
                 (assert (= r:exit 0)
                         (string "multi: ordered whole-file run must pass, got "
                                 r:exit " — stderr: " r:err))
                 (let [res (rows r:db
                                 (string "SELECT COUNT(DISTINCT form.hash) AS c "
                                 "FROM form WHERE form.file LIKE '%multi.lisp'"))]
                   (assert (= (get (get res 0) :c) 1)
                           (string "multi: a multi-form file is ONE form, got "
                                   (get (get res 0) :c) " distinct forms")))
                 (let [res (select-results r:db "multi.lisp"
                       (string "result.tier AS tier, "
                               "result.status AS status, " "form.label AS label"))]
                   (assert (= (row-tiers res) (tier-list script-tiers))
                           (string "multi: one row per JIT policy, got "
                                   (row-tiers res)))
                   (each row res
                     (assert (= row:status "pass")
                             (string "multi: expected pass, got " row:status
                                     " (per-form slicing reorders read before write)"))
                     (assert (= row:label "ordered read-after-write")
                             (string "multi: label scavenged from the first assert, got "
                                     row:label)))))

               # ── Scenario 6b: a multi-form file is ATOMIC — first failure aborts it ────
               # A legacy file's first failing assert aborts it, as a direct run does, and
               # the file records one :failed-assertion.
               (eprintln "scenario: multi-form whole-file (atomic abort)")
               (let [r (run-test "atomic" @[(fixture "atomic.lisp")])]
                 (assert (not (= r:exit 0))
                         (string "atomic: a failing file makes the gate exit nonzero, got "
                                 r:exit " — stderr: " r:err))
                 (let [res (select-results r:db "atomic.lisp"
                       (string "result.status AS status, "
                               "result.signal AS signal, "
                               "form.label AS label, " "result.tier AS tier"))]
                   (assert (= (row-tiers res) (tier-list script-tiers))
                           (string "atomic: one result per JIT policy, got "
                                   (row-tiers res)))
                   (each row res
                     (assert (= row:status "fail")
                             (string "atomic: expected fail, got " row:status))
                     (assert (= row:signal ":failed-assertion")
                             (string "atomic: expected :failed-assertion, got "
                                     row:signal))
                     (assert (= row:label "first failure aborts the file")
                             (string "atomic: label is the FIRST assert message, got "
                                     row:label)))))

               # ── Scenario 6c: the runner sets each JIT policy it records ────────────────
               # policy.lisp asserts that `(vm/config :jit)` reads nil or 0. A runner that
               # labelled its rows `vm` and `jit` without setting the policy leaves the
               # build's threshold, and the file fails under both.
               (eprintln "scenario: whole-file per-policy sets the JIT policy")
               (let [r (run-test "policy" @[(fixture "policy.lisp")])]
                 (assert (= r:exit 0)
                         (string "policy: the file observes the policy the runner set; exit="
                                 r:exit " stderr: " r:err))
                 (let [res (select-results r:db "policy.lisp"
                       "result.tier AS tier, result.status AS status")]
                   (assert (= (row-tiers res) (tier-list script-tiers))
                           (string "policy: one row per JIT policy, got "
                                   (row-tiers res)))
                   (each row res
                     (assert (= row:status "pass")
                             (string "policy: tier " row:tier " status "
                                     row:status)))))

               # ── Scenario 6d: a script whose value cannot leave its worker ─────────────
               # orphan.lisp ends on a fiber, which os/join cannot hand back, so the
               # runner runs the script again in its own process under each JIT policy,
               # and puts its own JIT setting back after each run. The counter-factual is
               # a runner that cannot restore the threshold it read once the JIT is off:
               # the run dies there, and the file after it never runs.
               (eprintln "scenario: a whole-file script whose value cannot leave its worker")
               (let [r (run-test "orphan"
                                 @[(fixture "orphan.lisp") (fixture "pass.lisp")])]
                 (assert (= r:exit 0)
                         (string "orphan: the in-process run passes the run; exit "
                                 r:exit " — stderr: " r:err))
                 (let [res (select-results r:db "orphan.lisp"
                       "result.tier AS tier, result.status AS status")]
                   (assert (= (row-tiers res) (tier-list script-tiers))
                           (string "orphan: one row per JIT policy, got "
                                   (row-tiers res)))
                   (each row res
                     (assert (= row:status "pass")
                             (string "orphan: tier " row:tier " status "
                                     row:status))))
                 (let [res (select-results r:db "pass.lisp"
                       "result.tier AS tier")]
                   (assert (= (row-tiers res) (tier-list form-tiers))
                           (string "orphan: the file after it runs on every tier, got "
                                   (row-tiers res)))))

               (eprintln "all per-tier runner scenarios passed"))
