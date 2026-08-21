(elle/epoch 12)
## Acceptance tests for `elle test` — the agent-first runner (docs/test-runner.md).
##
## These DRIVE the runner as a subprocess and assert on the SQLite session DB it
## writes. Written tests-first (docs -> tests -> code): they are EXPECTED TO FAIL
## until the runner subcommand, the `assert` macro payload, and `gate!`/`backend?`
## exist. The first failure today is `elle test` being an unknown subcommand
## (`test: No such file or directory`), which makes the harness itself valid.
##
## NOT part of `make smoke` — it spawns subprocesses and is quarantined in
## tests/runner/ until the runner is green, at which point it joins CI.
##
## Run directly:
##   ELLE=./target/debug/elle ./target/debug/elle tests/runner/acceptance.lisp
##
## Each scenario records a contract from the design doc. Exact text renderings
## (keyword spelling, predicate pretty-printing) are CONTRACT-DEFINING here: the
## implementation must match these assertions, not the other way around.

(def elle
  (or (get (sys/env) "ELLE")
      (if (file-exists? "./target/release/elle")
        "./target/release/elle"
        "./target/debug/elle")))
(def sqlite ((import "std/sqlite")))
(def fixtures "tests/runner/fixtures")

# Invoke `elle test --db DB EXTRA...` against an EXISTING session DB (no wipe).
(defn elle-test [db extra]
  (let [r (subprocess/system elle (concat @["test" "--db" db] extra))]
    {:exit r:exit :out r:stdout :err r:stderr :db db}))

# Same, but start a FRESH session first (clear the DB). Use for single-shot runs.
(defn run-test [db extra]
  (subprocess/system "sh" ["-c" (string "rm -f '" db "'")])
  (elle-test db extra))

(defn rows [db sql]
  (let [c (sqlite:open db)
        r (sqlite:query c sql)]
    (sqlite:close c)
    r))

(defn fixture [name]
  (string fixtures "/" name))

# ── Scenario 1: a passing form is `pass` on every tier ──────────────────────
(eprintln "scenario: pass")
(let [r (run-test "target/rt-pass.db" @[(fixture "pass.lisp")])]
  (assert (= r:exit 0)
          (string "pass: run-to-completion all-green exits 0, got " r:exit
                  " — stderr: " r:err))
  (let [res (rows r:db
                  (string "SELECT result.tier AS tier, result.status AS status "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%pass.lisp'"))]
    (assert (> (length res) 0) "pass: expected at least one result row")
    (each row res
      (assert (= row:status "pass")
              (string "pass: tier " row:tier " expected status=pass, got "
                      row:status)))))

# ── Scenario 2: a failing form — status, signal, and assert-macro payload ───
(eprintln "scenario: fail + assert payload")
(let [r (run-test "target/rt-fail.db" @[(fixture "fail.lisp")])]
  (assert (not (= r:exit 0)) "fail: any failure must make the gate exit nonzero")
  (let [res (rows r:db
                  (string "SELECT result.status AS status, result.signal AS signal, "
                          "result.syntax AS syntax, result.expected AS expected, "
                          "result.actual AS actual, form.label AS label "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%fail.lisp' LIMIT 1"))]
    (assert (> (length res) 0) "fail: expected a result row")
    (let [row (get res 0)]
      (assert (= row:status "fail") (string "fail: status=" row:status))
      (assert (= row:signal ":failed-assertion")
              (string "fail: signal=" row:signal))
      (assert (= row:label "wrong sum") (string "fail: label=" row:label))

      # the assert macro captured the predicate syntax, unevaluated, as data:
      (assert (= row:syntax "(= (+ 1 1) 3)") (string "fail: syntax=" row:syntax))

      # recognized comparison (= LHS RHS) -> actual=LHS value,
      # expected=RHS value:
      (assert (= row:actual "2") (string "fail: actual=" row:actual))
      (assert (= row:expected "3") (string "fail: expected=" row:expected)))))

# ── Scenario 3: a gated form skips on the wrong tier, runs on the right one ─
(eprintln "scenario: gate! skip")
(let [r (run-test "target/rt-gated.db" @[(fixture "gated.lisp")])]
  (assert (= r:exit 0)
          (string "gated: a skip is not a failure; exit " r:exit " — stderr: "
                  r:err))
  (let [res (rows r:db
                  (string "SELECT result.tier AS tier, result.status AS status, "
                          "result.reason AS reason "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%gated.lisp'"))]
    (assert (> (length res) 0) "gated: expected result rows")
    (each row res
      (if (= row:tier "vm")
        (begin
          (assert (= row:status "skip")
                  (string "gated: vm tier expected skip, got " row:status))
          (assert (= row:reason "needs JIT")
                  (string "gated: vm skip reason=" row:reason)))
        (if (= row:tier "jit")
          (assert (= row:status "pass")
                  (string "gated: jit tier expected pass, got " row:status))
          nil)))))

# ── Scenario 4: a tier-divergent value is recorded as `diverge` ─────────────
(eprintln "scenario: divergence")
(let [r (run-test "target/rt-diverge.db" @[(fixture "diverge.lisp")])]
  (let [res (rows r:db
                  (string "SELECT count(*) AS c "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%diverge.lisp' "
                          "AND result.status = 'diverge'"))]
    (assert (> (length res) 0) "diverge: expected a count row")
    (assert (>= (get (get res 0) :c) 1)
            "diverge: a form returning tier-dependent values must record a diverge row")))

# ── Scenario 5: ad-hoc `-e` form persists in the session as origin=:adhoc ───
(eprintln "scenario: ad-hoc persistence")
(let [r (run-test "target/rt-adhoc.db" @["-e" "(assert (= 1 1) \"adhoc ok\")"])]
  (assert (= r:exit 0)
          (string "adhoc: passing probe exits 0, got " r:exit " — stderr: "
                  r:err))
  (let [res (rows r:db
                  (string "SELECT origin AS origin, label AS label FROM form "
                          "WHERE origin = ':adhoc'"))]
    (assert (> (length res) 0)
            "adhoc: expected an :adhoc form row in the session")
    (let [row (get res 0)]
      (assert (= row:origin ":adhoc") (string "adhoc: origin=" row:origin))
      (assert (= row:label "adhoc ok") (string "adhoc: label=" row:label)))))

# ── Scenario 6: ad-hoc form promotes to a durable, flat .lisp file ──────────
# Promotion writes <corpus>/<name>.lisp. The corpus root is configurable (like
# --db), so we point it at a disposable, gitignored root under target/ — no
# pollution of the live corpus. Note: the promote step reuses the SAME session
# DB (elle-test, not run-test) so the ad-hoc form is still there to render.
(eprintln "scenario: ad-hoc -> promote round-trip")
(let [db "target/rt-promote.db"
      root "target/rt-corpus"]
  (subprocess/system "sh" ["-c" (string "rm -rf '" root "'")])
  (run-test db @["-e" "(assert (= 2 2) \"promote me\")"])
  (let [ids (rows db
                  "SELECT hash AS hash FROM form WHERE origin = ':adhoc' LIMIT 1")]
    (assert (> (length ids) 0) "promote: need an ad-hoc form to promote")
    (let [id (get (get ids 0) :hash)
          out (string root "/rt_promoted.lisp")
          pr (elle-test db @["--corpus" root "--promote" id "rt_promoted"])]
      (assert (= pr:exit 0)
              (string "promote: exit " pr:exit " — stderr: " pr:err))
      (assert (file-exists? out)
              (string "promote: expected " out " to be written")))))

# ── Scenario 7: a multi-form file is ONE whole-file thunk, run in order ──────
# Whole-file compilation (compile/whole-module): a legacy multi-form file runs as
# a single thunk, in source order, in isolation — a direct run. multi.lisp is the
# counter-factual for the old per-form slicing, which hoisted `def`/`var` ahead of
# the bare-expression forms so `(def snap (get cell 0))` ran BEFORE the
# `(put cell 0 …)` write and read pre-write garbage. As one thunk the write
# precedes the read; the file is ONE form (vm pass), labelled by its first assert.
(eprintln "scenario: multi-form whole-file (ordered)")
(let [r (run-test "target/rt-multi.db" @[(fixture "multi.lisp")])]
  (assert (= r:exit 0)
          (string "multi: ordered whole-file run must pass (gate exit 0), got "
                  r:exit " — stderr: " r:err))

  # exactly ONE form row for the whole file (not one per top-level form)
  (let [res (rows r:db
                  (string "SELECT COUNT(DISTINCT form.hash) AS c "
                          "FROM form WHERE form.file LIKE '%multi.lisp'"))]
    (assert (= (get (get res 0) :c) 1)
            (string "multi: a multi-form file is ONE form, got "
                    (get (get res 0) :c) " distinct forms")))  # the single form passes on vm; under per-form slicing the read-before-write
  # reorder would make "ordered read-after-write" fail with snap=0
  (let [res (rows r:db
                  (string "SELECT result.status AS status, form.label AS label "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%multi.lisp' AND result.tier = 'vm'"))]
    (assert (> (length res) 0) "multi: expected a vm row for the file")
    (assert (= (get (get res 0) :status) "pass")
            (string "multi: whole-file vm run expected pass, got "
                    (get (get res 0) :status)
                    " (per-form slicing reorders read before write)"))
    (assert (= (get (get res 0) :label) "ordered read-after-write")
            (string "multi: label scavenged from the first assert, got "
                    (get (get res 0) :label)))))

# ── Scenario 7b: a multi-form file is ATOMIC — first failure aborts the rest ─
# The deliberate trade of whole-file mode: a legacy file's first failing assert
# aborts it (a direct run), recorded as ONE :failed-assertion. The old per-form
# non-abort isolation — which is what reordered ordered scripts — is gone here.
(eprintln "scenario: multi-form whole-file (atomic abort)")
(let [r (run-test "target/rt-atomic.db" @[(fixture "atomic.lisp")])]
  (assert (not (= r:exit 0))
          (string "atomic: a failing file must make the gate exit nonzero, got "
                  r:exit " — stderr: " r:err))
  (let [res (rows r:db
                  (string "SELECT result.status AS status, result.signal AS signal, "
                          "form.label AS label "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%atomic.lisp' AND result.tier = 'vm'"))]
    (assert (= (length res) 1)
            (string "atomic: expected exactly ONE vm result for the file, got "
                    (length res)))
    (let [row (get res 0)]
      (assert (= (get row :status) "fail")
              (string "atomic: expected fail, got " (get row :status)))
      (assert (= (get row :signal) ":failed-assertion")
              (string "atomic: expected :failed-assertion, got "
                      (get row :signal)))
      (assert (= (get row :label) "first failure aborts the file")
              (string "atomic: label is the FIRST assert message, got "
                      (get row :label))))))

# ── Scenario 7c: a whole-file file runs under each JIT POLICY (vm + jit) ─────
# A legacy multi-form file is run once under :off (recorded "vm") and once under
# :eager (recorded "jit") — the old smoke-vm + smoke-jit split, folded into one
# run (process-whole / whole-file-policies, set per-worker via vm/config). No
# value-divergence is judged across policies: a script's pids/timestamps differ
# run-to-run by design. (Assumes the default build, which carries the JIT.)
(eprintln "scenario: multi-form whole-file per-policy (vm + jit)")
(let [r (run-test "target/rt-multi-policy.db" @[(fixture "multi.lisp")])]
  (assert (= r:exit 0)
          (string "multi-policy: must pass, got " r:exit " — stderr: " r:err))
  (let [res (rows r:db
                  (string "SELECT result.tier AS tier, result.status AS status "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%multi.lisp' "
                          "AND result.tier IN ('vm', 'jit') ORDER BY result.tier"))]
    (assert (= (length res) 2)
            (string "multi-policy: expected a vm AND a jit row (one per JIT "
                    "policy), got " (length res)))
    (assert (= (get (get res 0) :tier) "jit")
            "multi-policy: expected a jit (eager-policy) row")
    (assert (= (get (get res 1) :tier) "vm")
            "multi-policy: expected a vm (off-policy) row")
    (each row res
      (assert (= row:status "pass")
              (string "multi-policy: " row:tier " expected pass, got "
                      row:status)))))

# ── Scenario 7d: the per-policy run actually SETS the VM's JIT policy ────────
# policy.lisp asserts (vm/config :jit) is :off or :eager — never the default
# :adaptive. If the runner only *labelled* the rows vm/jit without setting the
# policy (e.g. a no-op `(put (vm/config) …)`), the file sees :adaptive and fails
# on both tiers. So both rows passing proves the policy was genuinely applied.
(eprintln "scenario: whole-file per-policy actually sets the VM policy")
(let [r (run-test "target/rt-policy.db" @[(fixture "policy.lisp")])]
  (assert (= r:exit 0)
          (string "policy: file asserts its own JIT policy is :off/:eager; a "
                  "nonzero exit means the runner did not set it. exit=" r:exit
                  " stderr: " r:err))
  (let [res (rows r:db
                  (string "SELECT result.tier AS tier, result.status AS status "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%policy.lisp' "
                          "AND result.tier IN ('vm', 'jit')"))]
    (assert (= (length res) 2)
            (string "policy: expected vm + jit rows, got " (length res)))
    (each row res
      (assert (= row:status "pass")
              (string "policy: " row:tier
                      " row failed — runner ran the file under :adaptive (label "
                      "only), not the per-tier policy")))))

# ── Scenario 8: a file that won't compile is one file-level failure ─────────
(eprintln "scenario: compile error is file-level")
(let [r (run-test "target/rt-broken.db" @[(fixture "broken.lisp")])]
  (assert (not (= r:exit 0))
          (string "broken: a non-compiling file must make the gate exit "
                  "nonzero, got " r:exit))
  (let [res (rows r:db
                  (string "SELECT result.status AS status "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%broken.lisp'"))]
    (assert (>= (length res) 1)
            "broken: expected at least one file-level result row")
    (each row res
      (assert (or (= row:status "fail") (= row:status "error"))
              (string "broken: file-level row expected fail/error, got "
                      row:status)))))

# ── Scenario 9: compile --dump artifacts are OMITTED from the runner ─────────
# The per-file (compile/dumps …) pass is the largest contributor to the corpus
# region leak that OOMs `make smoke`, and the dumps are not byte-deterministic
# (absolute @-HirIds), so they would not even CAS-dedup. Until the leak is fixed
# the runner captures NO --dump artifacts: a passing, non-printing form leaves
# ZERO asset rows of any dump kind (and no `lir` headline artifact in particular).
# Counter-factual: fails on a dump-capturing binary, which writes lir/ast/… rows.
# (See docs/test-runner.md § CAS asset capture status note.)
(eprintln "scenario: --dump capture omitted")
(def compress ((import "std/compress")))
(defn read-cas-bytes [hash]
  (let [p (port/open-bytes (string "target/cas/" hash) :read)
        b (port/read p 4000000)]
    (port/close p)
    b))

# The --dump kinds capture-dumps used to emit (stdout/stderr are NOT dumps).
(def dump-kinds
  ["ast" "fhir" "defuse" "regions" "hir" "lir" "cfg" "dfa" "jit" "escape"])

# Build a SQL `'a','b',...` literal list from a vector of strings.
(defn sql-quote [s]
  (string "'" s "'"))
(defn sql-in-list [items]
  (string/join (map sql-quote items) ","))

# How many dump-kind asset rows the run recorded for FILE (a LIKE pattern).
(defn count-dump-assets [db file]
  (let [join "JOIN result ON result.id = asset.result_id "
        join2 "JOIN form ON form.hash = result.form_hash "
        where (string "WHERE form.file LIKE '" file "' " "AND asset.kind IN ("
                      (sql-in-list dump-kinds) ")")
        sql (string "SELECT count(*) AS n FROM asset " join join2 where)]
    (get (get (rows db sql) 0) :n)))

# A passing, non-printing form records ZERO dump-kind assets — no `lir`
# headline artifact, no `ast`, nothing — because capture-dumps is a no-op.
(let [r (run-test "target/rt-assets.db" @[(fixture "pass.lisp")])]
  (assert (= r:exit 0)
          (string "assets: pass run exits 0, got " r:exit " — stderr: " r:err))
  (let [n (count-dump-assets r:db "%pass.lisp")]
    (assert (= n 0) (string "assets: expected NO dump asset rows, got " n))))

# ── Scenario 10: a test's stdout/stderr are captured to the CAS ─────────────
# Per (form × tier) the runner rebinds *stdout*/*stderr* (under a worker-side
# ev/run, riding the bundle now that parameters are sendable) and records any
# non-empty output as `stdout`/`stderr` assets. Counter-factual: a printing test
# could not even run in a worker on the pre-change binary ("Cannot send
# parameter", then "Unexpected yield outside coroutine context").
(eprintln "scenario: stdout/stderr capture")
(let [r (run-test "target/rt-print.db" @[(fixture "print.lisp")])]
  (assert (= r:exit 0)
          (string "print: form prints then passes; exit " r:exit " — stderr: "
                  r:err))
  (defn stdio-asset [db kind]
    (rows db
          (string "SELECT asset.hash AS hash, asset.size AS size, "
                  "asset.codec AS codec " "FROM asset "
                  "JOIN result ON result.id = asset.result_id "
                  "JOIN form ON form.hash = result.form_hash "
                  "WHERE form.file LIKE '%print.lisp' AND result.tier = 'vm' "
                  "AND asset.kind = '" kind "' LIMIT 1")))
  (each spec [["stdout" "hello stdout"] ["stderr" "hello stderr"]]
    (let [kind (get spec 0)
          want (get spec 1)
          res (stdio-asset r:db kind)]
      (assert (> (length res) 0)
              (string "print: expected a '" kind "' asset on the vm result"))
      (let [row (get res 0)
            hash (get row :hash)]
        (assert (= (get row :codec) "zstd")
                (string "print: " kind " codec=" (get row :codec)))
        (assert (file-exists? (string "target/cas/" hash))
                (string "print: expected CAS file target/cas/" hash))
        (let [raw (compress:unzstd (read-cas-bytes hash))]
          (assert (= (length raw) (get row :size))
                  (string "print: " kind " size mismatch"))
          (assert (string/contains? (string raw) want)
                  (string "print: captured " kind " must contain '" want
                          "', got: " (string raw))))))))

# ── Scenario 11: a hung form is bounded by --timeout, recorded `timeout` ─────
# A test whose worker never finishes must NOT wedge the run. os/join with a
# deadline records the form `timeout` (distinct from pass/fail/skip), the run
# returns promptly at the deadline (not after the worker would finish), and the
# gate exits non-zero. The runaway worker is abandoned, not killed (§ Isolation).
# The fixture sleeps far longer than the deadline, so a working timeout returns
# in well under the sleep; a broken one would wait the whole sleep out.
(eprintln "scenario: per-test timeout")
(let [r (run-test "target/rt-timeout.db"
                  @["--timeout" "500" "-e" "(ev/sleep 10)"])]
  (assert (not (= r:exit 0))
          (string "timeout: a timed-out form must gate non-zero; exit " r:exit))
  (let [res (rows r:db
                  "SELECT tier, status, reason FROM result WHERE status = 'timeout'")]
    (assert (> (length res) 0)
            "timeout: expected a result row with status=timeout")
    (let [row (get res 0)]
      (assert (string/contains? (string row:reason) "deadline")
              (string "timeout: reason=" row:reason))))
  (let [run (rows r:db "SELECT n_timeout FROM run")]
    (assert (>= (get (get run 0) :n_timeout) 1)
            "timeout: run.n_timeout must count the timed-out form")))

# ── Scenario 12: a gated SHARED SETUP skips the file (not a file-level fail) ──
# When a file's eager (def …) setup raises :gated (an absent optional dependency,
# e.g. a missing FFI library re-raised as :gated), the compile aborts before any
# test thunk is built — so the runner records it exactly like a file-level
# compile error, but as a SKIP: one row (form_index -1) with status=skip and the
# reason, and exit 0. This is what lets FFI/service tests self-gate instead of
# being name-skipped in the Makefile, and replaces the dangerous (sys/exit 0)
# idiom (which under the runner terminates the process mid-run, silently dropping
# later forms). A genuine setup error stays a file-level FAIL (scenario 8).
(eprintln "scenario: gated shared setup")
(let [r (run-test "target/rt-gated-setup.db" @[(fixture "gated-setup.lisp")])]
  (assert (= r:exit 0)
          (string "gated-setup: a gated setup skips (not fails); exit " r:exit
                  " — stderr: " r:err))  # Whole-file mode: the gate runs INSIDE the file's single thunk, so the vm row
  # is a runtime :gated skip carrying the reason (idx 0, the whole-file form) —
  # not the old eager-setup file-level skip (idx -1).
  (let [res (rows r:db
                  (string "SELECT result.status AS status, result.reason AS reason, "
                          "form.form_index AS idx "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%gated-setup.lisp' AND result.tier = 'vm'"))]
    (assert (= (length res) 1)
            (string "gated-setup: expected one vm row, got " (length res)))
    (let [row (get res 0)]
      (assert (= row:status "skip")
              (string "gated-setup: expected skip, got " row:status))
      (assert (= row:reason "libfixture.so not installed")
              (string "gated-setup: reason=" row:reason))
      (assert (= row:idx 0)
              (string "gated-setup: the gated file is its own whole-file form "
                      "(idx 0), got " row:idx))))  # The run gate counts the skip, and nothing failed.
  (let [run (rows r:db "SELECT n_fail AS f, n_skip AS s FROM run")]
    (assert (= (get (get run 0) :f) 0) "gated-setup: n_fail must be 0")
    (assert (>= (get (get run 0) :s) 1)
            (string "gated-setup: n_skip must count the gated file, got "
                    (get (get run 0) :s)))))

# ── Scenario 13: a form capturing an UNSENDABLE value runs in-process ───────
# A test thunk that closes over a value which can't cross os/spawn (here a
# fiber; in the corpus: FFI handles, compile/* artifacts, arena values) can't
# run in a worker — the spawn raises a serialization :thread-error. The runner
# must fall back to running that form IN-PROCESS rather than record a spurious
# fail. The sendable form in the same file keeps the normal worker path. Both
# pass, exit 0. Counter-factual: before the fallback, the fiber-capturing form
# was a :thread-error fail.
(eprintln "scenario: unsendable capture runs in-process")
(let [r (run-test "target/rt-unsendable.db" @[(fixture "unsendable.lisp")])]
  (assert (= r:exit 0)
          (string "unsendable: in-process fallback must make the run pass; exit "
                  r:exit " — stderr: " r:err))
  (let [res (rows r:db
                  (string "SELECT result.tier AS tier, result.status AS status, "
                          "result.signal AS signal, form.label AS label "
                          "FROM result JOIN form ON form.hash = result.form_hash "
                          "WHERE form.file LIKE '%unsendable.lisp'"))]
    (assert (> (length res) 0) "unsendable: expected result rows")  # No row is a :thread-error fail — the unsendable form ran in-process.
    (each row res
      (assert (not (= row:signal ":thread-error"))
              (string "unsendable: form '" row:label "' still :thread-error on "
                      row:tier " (in-process fallback missing)"))
      (assert (not (= row:status "fail"))
              (string "unsendable: form '" row:label "' failed on " row:tier)))  # The fiber-capturing form is present and passed on the vm tier.
    (let [vmrows (rows r:db
                       (string "SELECT result.status AS status "
                               "FROM result JOIN form ON form.hash = result.form_hash "
                               "WHERE form.file LIKE '%unsendable.lisp' "
                               "AND form.label LIKE '%runs in-process%' "
                               "AND result.tier = 'vm'"))]
      (assert (> (length vmrows) 0)
              "unsendable: expected the fiber-capturing form's vm row")
      (assert (= (get (get vmrows 0) :status) "pass")
              (string "unsendable: fiber-capturing form must pass in-process, got "
                      (get (get vmrows 0) :status))))))

# ── Scenario 13: a run RENDERS its results (no hand-written SQLite) ──────────
# Every run prints a tally to stderr, plus a problem line per non-pass form, so
# you read the outcome from the run itself. --summary re-renders an existing DB,
# and --query runs ad-hoc SQL to stdout. Counter-factual: the runner used to exit
# with only a code and print nothing of its own.
(eprintln "scenario: run prints a summary; --summary and --query inspect the DB")
(let [r (run-test "target/rt-summary.db" @[(fixture "fail.lisp")])]
  (assert (not (= r:exit 0)) "summary: a failing run still gates nonzero")  # the end-of-run tally line is on stderr
  (assert (string/contains? r:err "elle test")
          (string "summary: expected a tally line on stderr, got: " r:err))  # a failing form is named in the problem list, with its status
  (assert (string/contains? r:err "fail")
          (string "summary: expected the failing form listed, got: " r:err))  # --summary re-renders the SAME run from the DB, no re-run
  (let [s (elle-test r:db @["--summary"])]
    (assert (= s:exit 0) "summary: --summary exits 0 (it only reads)")
    (assert (string/contains? s:err "elle test")
            (string "summary: --summary must print the tally, got: " s:err)))  # --query runs arbitrary SQL to stdout
  (let [q (elle-test r:db @["--query" "SELECT count(*) AS n FROM result"])]
    (assert (= q:exit 0) "summary: --query exits 0")
    (assert (string/contains? q:out "n")
            (string "summary: --query must print rows to stdout, got: " q:out))))

(eprintln "all runner acceptance scenarios passed")
