(elle/epoch 18)
## audited: 2026-11-23
## The helpers the runner's acceptance tests share: the binary under test and
## the rig beside it, a run's DB read back, and the tiers a run records.
## tests/runner/overview.md
## docs/test-runner.md
##
## Each acceptance test splices this file with the top-level
## `include-file` directive (docs/include.md). The implementation suite runs
## only the files at the top of tests/runner, so this file never runs as a
## test itself.

# The binary under test is the `elle` beside the executable that runs this
# file, so a rig tests the `elle` of its own build (tests/runner/overview.md).
(def elle
  (let [exe (elle/executable)]
    (assert (not (nil? exe)) "the OS named no running executable")
    (path/absolute (path/join (path/parent exe) "elle"))))
(def sqlite ((import "std/sqlite")))
(def compress ((import "std/compress")))
(def fixtures "tests/runner/fixtures")

(defn fixture [name]
  (string fixtures "/" name))

(defn write-fixture [dir name src]
  "Write SRC as DIR/NAME, and answer the path."
  (let [path (path/join dir name)]
    (file/write path src)
    path))

(defn rows [db sql]
  (let [c (sqlite:open db)
        r (sqlite:query c sql)]
    (sqlite:close c)
    r))

(defn scalar [db sql]
  "The column `c` of the first row SQL answers from DB."
  (get (get (rows db sql) 0) :c))

# The latest run's id, for a query to compare a row's run against.
(def latest-run "(SELECT max(id) FROM run)")

(defn select-results [db file cols]
  (rows db
        (string "SELECT " cols " FROM result "
                "JOIN form ON form.hash = result.form_hash "
                "WHERE form.file LIKE '%" file "'")))

(defn run-binary [exe args &named env cwd]
  "Run EXE with ARGS, and answer what it left. ENV replaces the environment,
   and CWD is the working directory; nil inherits each."
  (let [r (subprocess/system exe args {:env env :cwd cwd})]
    {:exit r:exit :out r:stdout :err r:stderr}))

(defn elle-run [args &named env cwd]
  "Run the binary under test with ARGS, as run-binary does."
  (run-binary elle args :env env :cwd cwd))

(defn elle-feed [args input &named env cwd deadline]
  "Run the binary under test with ARGS, write INPUT to its standard input and
   close it, and answer what it left, as elle-run does. Both streams drain
   while the child runs, so a child that fills a pipe cannot wedge. Past
   DEADLINE seconds, when one is named, the child is killed and :exit is nil."
  (let [p (subprocess/exec elle args {:env env :cwd cwd})
        out (ev/spawn (fn [] (string (port/read-all (get p :stdout)))))
        err (ev/spawn (fn [] (string (port/read-all (get p :stderr)))))]
    (port/write (get p :stdin) input)
    (port/close (get p :stdin))
    (let [code (if deadline
                 (or (ev/timeout deadline (fn [] (subprocess/wait p)))
                     (begin
                       (subprocess/kill p :sigkill)
                       (subprocess/wait p)
                       nil))
                 (subprocess/wait p))]
      {:exit code :out (ev/join out) :err (ev/join err)})))

# Invoke `elle test --db DB EXTRA...` against an existing session DB.
(defn elle-test [db extra &named env cwd]
  (put (elle-run ["test" "--db" db ;extra] :env env :cwd cwd) :db db))

# The rig beside the binary under test, for a scenario that needs a primitive
# only a rig carries (rig/overview.md).
(def elle-rig (path/join (path/parent elle) "elle-rig"))

(defn rig-test [db extra]
  "`elle-rig test --db DB EXTRA...`, answered as elle-test answers."
  (put (run-binary elle-rig ["test" "--db" db ;extra]) :db db))

(defn query! [db sql]
  "Run SQL through `elle test --query`, which makes the store when DB holds
   none, and answer what it printed. A statement that fails stops the test:
   an empty answer reads the same as a failed one."
  (let [r (elle-test db ["--query" sql])]
    (assert (= r:exit 0) (string "the query failed: " sql " — " r:err))
    r:out))

(defn env-without [& names]
  "This process's environment, less the variables NAMES."
  (fold (fn [e n] (del e n)) (sys/env) names))

(defn lines [text]
  "The lines of TEXT that hold more than blanks."
  (filter (fn [l] (not (empty? (string/trim l)))) (string/split text "\n")))

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
