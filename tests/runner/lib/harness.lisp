(elle/epoch 13)
## audited: 2026-09-29
## The helpers the runner's acceptance tests share: the binary under test, a
## run's DB read back, and the tiers a run records.
## tests/runner/overview.md
## docs/test-runner.md
##
## tiers.lisp and acceptance.lisp splice this file with the top-level
## `include-file` directive (docs/modules.md). The implementation suite runs
## only the files it names, so this file never runs as a test itself.

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

# Invoke `elle test --db DB EXTRA...` against an existing session DB.
(defn elle-test [db extra]
  (let [r (subprocess/system elle (concat @["test" "--db" db] extra))]
    {:exit r:exit :out r:stdout :err r:stderr :db db}))

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
