(elle/epoch 13)
## audited: 2026-09-29
## Acceptance tests for `elle test`: each scenario drives the runner as a
## subprocess and asserts on the session DB it writes.
## docs/test-runner.md
##
## The implementation suite runs this file (`make smoke-impl`). `ELLE` names the
## binary under test; the Makefile exports it.
##
## Every scenario writes its DB, its CAS and its scratch files under one
## directory from `with-temp-dir`, so two runs never share a path and nothing
## survives the run.
##
## Exact text renderings (keyword spelling, predicate pretty-printing) are part
## of the contract: the runner must match these assertions.

(def elle
  (path/absolute (or (get (sys/env) "ELLE")
                     (if (file-exists? "./target/release/elle")
                       "./target/release/elle"
                       "./target/debug/elle"))))
(def sqlite ((import "std/sqlite")))
(def compress ((import "std/compress")))
(def fixtures "tests/runner/fixtures")

(defn fixture [name]
  (string fixtures "/" name))

(defn rows [db sql]
  (let [c (sqlite:open db)
        r (sqlite:query c sql)]
    (sqlite:close c)
    r))

(defn select-results [db file cols]
  (rows db
        (string "SELECT " cols " FROM result "
                "JOIN form ON form.hash = result.form_hash "
                "WHERE form.file LIKE '%" file "'")))

# The tiers the runner records for a single form: every tier this build
# carries, probed the way the runner probes them (docs/test-runner.md).
# `:bytecode` is recorded as `vm`.
(defn carried? [tier]
  (let [[ok? err] (protect (compile/run-on tier (fn [] 1)))]
    (or ok? (not (= (get err :reason) :feature-disabled)))))
(defn tier-label [tier]
  (if (= tier :bytecode) "vm" (string tier)))
(def form-tiers
  (map tier-label (filter carried? [:bytecode :jit :wasm :mlir-cpu])))
(def jit? (carried? :jit))

# The tiers the runner records for a whole-file script: the JIT off, and the
# JIT eager where the build carries it.
(def script-tiers (if jit? ["vm" "jit"] ["vm"]))

(defn tier-list [tiers]
  "TIERS sorted and joined, so two lists of tier names compare as strings."
  (string/join (sort (map string tiers)) ","))

(defn row-tiers [res]
  (tier-list (map (fn [row] row:tier) res)))

(with-temp-dir dir
               # Invoke `elle test --db DB EXTRA...` against an existing session DB.
               (defn elle-test [db extra]
                 (let [r (subprocess/system elle
                       (concat @["test" "--db" db] extra))]
                   {:exit r:exit :out r:stdout :err r:stderr :db db}))

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

               # ── Scenario 4: ad-hoc `-e` form persists in the session as origin=:adhoc ─
               (eprintln "scenario: ad-hoc persistence")
               (let [r (run-test "adhoc" @["-e" "(assert (= 1 1) \"adhoc ok\")"])]
                 (assert (= r:exit 0)
                         (string "adhoc: passing probe exits 0, got " r:exit
                                 " — stderr: " r:err))
                 (let [res (rows r:db
                                 (string "SELECT origin AS origin, label AS label FROM form "
                                 "WHERE origin = ':adhoc'"))]
                   (assert (> (length res) 0)
                           "adhoc: expected an :adhoc form row")
                   (let [row (get res 0)]
                     (assert (= row:origin ":adhoc")
                             (string "adhoc: origin=" row:origin))
                     (assert (= row:label "adhoc ok")
                             (string "adhoc: label=" row:label)))))

               # ── Scenario 5: ad-hoc form promotes to a durable, flat .lisp file ────────
               # Promotion writes <corpus>/<name>.lisp under a root the caller names. The
               # promote step reuses the same session DB, so the ad-hoc form is still there
               # to render.
               (eprintln "scenario: ad-hoc -> promote round-trip")
               (let [root (path/join dir "rt-corpus")
                     r (run-test "promote"
                                 @["-e" "(assert (= 2 2) \"promote me\")"])
                     ids (rows r:db
                               "SELECT hash AS hash FROM form WHERE origin = ':adhoc' LIMIT 1")]
                 (assert (> (length ids) 0)
                         "promote: need an ad-hoc form to promote")
                 (let [id (get (get ids 0) :hash)
                       out (path/join root "rt_promoted.lisp")
                       pr (elle-test r:db
                                     @["--corpus" root "--promote" id
                                       "rt_promoted"])]
                   (assert (= pr:exit 0)
                           (string "promote: exit " pr:exit " — stderr: "
                                   pr:err))
                   (assert (file-exists? out)
                           (string "promote: expected " out " to be written"))))

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

               # ── Scenario 7: a file that won't compile is one file-level failure ───────
               (eprintln "scenario: compile error is file-level")
               (let [r (run-test "broken" @[(fixture "broken.lisp")])]
                 (assert (not (= r:exit 0))
                         (string "broken: a non-compiling file makes the gate exit nonzero, "
                                 "got " r:exit))
                 (let [res (select-results r:db "broken.lisp"
                       "result.status AS status")]
                   (assert (>= (length res) 1)
                           "broken: expected a file-level result row")
                   (each row res
                     (assert (= row:status "fail")
                             (string "broken: file-level row expected fail, got "
                                     row:status)))))

               # ── Scenario 8: compile --dump artifacts are OMITTED from the runner ──────
               # The per-file (compile/dumps …) pass leaks regions and its dumps are not
               # byte-deterministic, so the runner captures no dump artifact: a passing,
               # non-printing form leaves no asset row of any dump kind
               # (docs/test-runner.md). Counter-factual: a
               # dump-capturing binary writes lir/ast/… rows.
               (eprintln "scenario: --dump capture omitted")
               (def dump-kinds
                 ["ast" "fhir" "defuse" "regions" "hir" "lir" "cfg" "dfa" "jit"
                  "escape"])
               (let [r (run-test "assets" @[(fixture "pass.lisp")])
                     kinds (string/join (map (fn [k] (string "'" k "'"))
                                        dump-kinds) ",")
                     res (rows r:db
                               (string "SELECT count(*) AS n FROM asset "
                                       "JOIN result ON result.id = asset.result_id "
                                       "JOIN form ON form.hash = result.form_hash "
                                       "WHERE form.file LIKE '%pass.lisp' "
                                       "AND asset.kind IN (" kinds ")"))]
                 (assert (= r:exit 0)
                         (string "assets: pass run exits 0, got " r:exit
                                 " — stderr: " r:err))
                 (assert (= (get (get res 0) :n) 0)
                         (string "assets: expected NO dump asset rows, got "
                                 (get (get res 0) :n))))

               # ── Scenario 9: a test's stdout/stderr are captured to the CAS ────────────
               # The worker rebinds *stdout*/*stderr* and records non-empty output as
               # `stdout`/`stderr` assets, zstd-compressed in the CAS beside the DB.
               (eprintln "scenario: stdout/stderr capture")
               (let [r (run-test "print" @[(fixture "print.lisp")])]
                 (assert (= r:exit 0)
                         (string "print: form prints then passes; exit " r:exit
                                 " — stderr: " r:err))
                 (each spec [["stdout" "hello stdout"] ["stderr" "hello stderr"]]
                   (let [kind (get spec 0)
                         want (get spec 1)
                         res (rows r:db
                                   (string "SELECT asset.hash AS hash, asset.size AS size, "
                                   "asset.codec AS codec FROM asset "
                                   "JOIN result ON result.id = asset.result_id "
                                   "JOIN form ON form.hash = result.form_hash "
                                   "WHERE form.file LIKE '%print.lisp' "
                                   "AND result.tier = 'vm' "
                                   "AND asset.kind = '" kind "' LIMIT 1"))]
                     (assert (> (length res) 0)
                             (string "print: expected a '" kind
                                     "' asset on the vm row"))
                     (let [row (get res 0)
                           cas (path/join dir "cas" row:hash)]
                       (assert (= row:codec "zstd")
                               (string "print: " kind " codec=" row:codec))
                       (assert (file-exists? cas)
                               (string "print: expected CAS file " cas))
                       (let [p (port/open-bytes cas :read)
                             raw (compress:unzstd (port/read p 4000000))]
                         (port/close p)
                         (assert (= (length raw) row:size)
                                 (string "print: " kind " size mismatch"))
                         (assert (string/contains? (string raw) want)
                                 (string "print: captured " kind
                                 " must contain '" want "', got: " (string raw))))))))

               # ── Scenario 10: a hung form is bounded by --timeout, recorded `timeout` ──
               # The fixture sleeps far longer than the deadline, so a working timeout
               # returns well before the sleep ends and the gate exits nonzero.
               (eprintln "scenario: per-test timeout")
               (let [r (run-test "timeout"
                                 @["--timeout" "500" "-e" "(ev/sleep 10)"])]
                 (assert (not (= r:exit 0))
                         (string "timeout: a timed-out form gates nonzero; exit "
                                 r:exit))
                 (let [res (rows r:db
                                 "SELECT tier, status, reason FROM result WHERE status = 'timeout'")]
                   (assert (> (length res) 0)
                           "timeout: expected a row with status=timeout")
                   (assert (string/contains? (string (get (get res 0) :reason))
                           "deadline")
                           (string "timeout: reason=" (get (get res 0) :reason))))
                 (let [run (rows r:db "SELECT n_timeout FROM run")]
                   (assert (>= (get (get run 0) :n_timeout) 1)
                           "timeout: run.n_timeout must count the timed-out form")))

               # ── Scenario 11: a gated SHARED SETUP skips the file ───────────────────────
               # The gate runs inside the file's single thunk, so the row is a runtime
               # :gated skip carrying the reason on the whole-file form (index 0). A genuine
               # setup error stays a failure (scenario 7).
               (eprintln "scenario: gated shared setup")
               (let [r (run-test "gated-setup" @[(fixture "gated-setup.lisp")])]
                 (assert (= r:exit 0)
                         (string "gated-setup: a gated setup skips; exit "
                                 r:exit " — stderr: " r:err))
                 (let [res (select-results r:db "gated-setup.lisp"
                       (string "result.status AS status, "
                               "result.reason AS reason, "
                               "form.form_index AS idx, " "result.tier AS tier"))]
                   (assert (= (row-tiers res) (tier-list script-tiers))
                           (string "gated-setup: one row per JIT policy, got "
                                   (row-tiers res)))
                   (each row res
                     (assert (= row:status "skip")
                             (string "gated-setup: expected skip, got "
                                     row:status))
                     (assert (= row:reason "libfixture.so not installed")
                             (string "gated-setup: reason=" row:reason))
                     (assert (= row:idx 0)
                             (string "gated-setup: the file is its own whole-file form "
                                     "(idx 0), got " row:idx))))
                 (let [run (rows r:db "SELECT n_fail AS f, n_skip AS s FROM run")]
                   (assert (= (get (get run 0) :f) 0)
                           "gated-setup: n_fail must be 0")
                   (assert (>= (get (get run 0) :s) 1)
                           (string "gated-setup: n_skip must count the gated file, got "
                                   (get (get run 0) :s)))))

               # ── Scenario 12: a form capturing an UNSENDABLE value runs in-process ─────
               # A thunk that closes over a value which cannot cross os/spawn (here a
               # fiber) raises a serialization :thread-error on spawn. The runner re-runs
               # it in-process instead of recording a spurious fail.
               (eprintln "scenario: unsendable capture runs in-process")
               (let [r (run-test "unsendable" @[(fixture "unsendable.lisp")])]
                 (assert (= r:exit 0)
                         (string "unsendable: the in-process fallback passes the run; exit "
                                 r:exit " — stderr: " r:err))
                 (let [res (select-results r:db "unsendable.lisp"
                       (string "result.status AS status, "
                               "result.signal AS signal, " "form.label AS label"))]
                   (assert (> (length res) 0) "unsendable: expected result rows")
                   (each row res
                     (assert (not (= row:signal ":thread-error"))
                             (string "unsendable: form '" row:label
                                     "' is still a :thread-error (fallback missing)"))
                     (assert (= row:status "pass")
                             (string "unsendable: form '" row:label "' got "
                                     row:status)))))

               # ── Scenario 13: a run RENDERS its results ─────────────────────────────────
               # Every run prints a tally to stderr, plus a problem line per non-pass form.
               # --summary re-renders an existing DB, and --query runs ad-hoc SQL.
               (eprintln "scenario: run prints a summary; --summary and --query inspect the DB")
               (let [r (run-test "summary" @[(fixture "fail.lisp")])]
                 (assert (not (= r:exit 0))
                         "summary: a failing run still gates nonzero")
                 (assert (string/contains? r:err "elle test")
                         (string "summary: expected a tally line on stderr, got: "
                                 r:err))
                 (assert (string/contains? r:err "fail")
                         (string "summary: expected the failing form listed, got: "
                                 r:err))
                 (let [s (elle-test r:db @["--summary"])]
                   (assert (= s:exit 0)
                           "summary: --summary exits 0 (it only reads)")
                   (assert (string/contains? s:err "elle test")
                           (string "summary: --summary must print the tally, got: "
                                   s:err)))
                 (let [q (elle-test r:db
                                    @["--query"
                                      "SELECT count(*) AS n FROM result"])]
                   (assert (= q:exit 0) "summary: --query exits 0")
                   (assert (string/contains? q:out "n")
                           (string "summary: --query must print rows to stdout, got: "
                                   q:out))))

               # ── Scenario 14: --host runs each child under another program ──────────────
               # The host records its argv, then runs the file under this elle. A runner
               # that ignored --host leaves no record; one that passed the flags or the path
               # wrongly leaves a record that says so.
               (eprintln "scenario: --host")
               (let [record (path/join dir "host-argv.txt")
                     host (path/join dir "host.sh")]
                 (file/write host
                             (string "#!/bin/sh\nprintf '%s\\n' \"$@\" > '"
                                     record "'\n" "exec '" elle "' \"$@\"\n"))
                 (subprocess/system "chmod" ["755" host])
                 (let [r (run-test "host"
                                   @["--host" host "--isolate" "--unicode=16.0"
                                     (fixture "pass.lisp")])]
                   (assert (= r:exit 0)
                           (string "host: the hosted child passes; exit " r:exit
                                   " — stderr: " r:err))
                   (assert (file-exists? record) "host: the host never ran")
                   (assert (= (slurp record)
                              (string "--unicode=16.0\n" (fixture "pass.lisp")
                                      "\n"))
                           (string "host: the host got " (slurp record)))
                   (let [res (select-results r:db "pass.lisp"
                         "result.tier AS tier, result.status AS status")]
                     (assert (= (length res) 1)
                             (string "host: one row, got " (length res)))
                     (assert (= (get (get res 0) :tier) "process")
                             (string "host: tier " (get (get res 0) :tier)))
                     (assert (= (get (get res 0) :status) "pass")
                             "host: expected pass"))))

               (eprintln "all runner acceptance scenarios passed"))
