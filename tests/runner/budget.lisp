(elle/epoch 18)
## audited: 2026-11-24
## The wall-clock budget `elle test` gives one form, and how the suite passes tell it which paths need a wider one.
## tests/runner/overview.md
## docs/test-cli.md
##
## A suite pass hands one runner process a whole batch of files, so no shell
## sees a path in time to choose a budget for it. The policy travels to the
## runner as flags instead. Three places have to agree, and nothing compiles any
## of them: the Makefile names the families, the recipe passes them, and the
## runner reads them when it sets a form's deadline. Each pair can be right while
## the third is wrong, and the failure is the same: a file is killed at a budget
## narrower than its own deadline, so the diagnostic it exists to print never
## prints, and CI reports a bare `timeout`.

(include-file "lib/harness.lisp")
(include-file "lib/make.lisp")

(def wide-flags (string/split (make-var "WIDE_FLAGS") " "))
(def wide-ms (parse-int (make-var "WIDE_TIMEOUT_MS")))
(def families (string/split (make-var "WIDE_FAMILIES") " "))

(defn suite [dir]
  "Every file at the top of the suite in DIR, as a repo-relative path."
  (let [paths (map (fn [n] (string dir "/" n))
                   (filter (fn [n] (string/ends-with? n ".lisp")) (file/ls dir)))]
    (assert (> (length paths) 100) (string "the suite in " dir " did not read"))
    paths))

(def suite-files [;(suite "tests/lang") ;(suite "tests/impl")])

(defn wide? [path]
  (any? (fn [f] (string/contains? path f)) families))

(defn deadline-of [path]
  "The deadline in seconds PATH gives itself with `(def deadline N)`, or nil."
  (let [src (slurp path)
        at (string/find src "(def deadline ")]
    (if (nil? at)
      nil
      (let [rest (slice src (+ at (length "(def deadline ")))]
        (parse-int (string/trim (slice rest 0 (string/find rest ")"))))))))

# `elle test --budget` prints a budget per path and exits without touching the
# store. Asking the binary is the point: the selector belongs to the thing
# under test, and a copy of its rule here is a second runner.
(defn budgets [flags paths]
  "The per-form budget the runner gives each of PATHS under FLAGS, in ms."
  (let [r (elle-run ["test" "--budget" ;flags ;paths])
        got (map (fn [l]
                   (let [words (string/split (string/trim l) " ")]
                     [(get words 1) (parse-int (get words 0))])) (lines r:out))]
    (assert (= r:exit 0) (string "`elle test --budget` failed: " r:out r:err))
    (assert (= (length got) (length paths))
            (string "`elle test --budget` answered for " (length got) " of "
                    (length paths) " paths: " r:out))
    got))

(def default-ms (get (first (budgets [] ["tests/lang/arithmetic.lisp"])) 1))

# A policy the recipe never passes is a policy the runner never hears. A batch
# line must also carry no bare `--timeout`: one wide budget for the whole batch
# clears every declared deadline, and it hands the wide budget to every
# ordinary file, so a hang the gate caught in a minute costs two and a half.
(eprintln "scenario: every suite pass hands the runner the wide policy")
(each target ["smoke-lang" "smoke-impl"]
  (let [batches (filter (fn [l]
                          (and (string/contains? l " test ")
                               (string/contains? l "xargs")))
                        (string/split (make-dry-run target) "\n"))]
    (assert (not (empty? batches))
            (string "`make " target "` runs no `elle test` batch"))
    (each batch batches
      (assert (string/contains? batch (string/join wide-flags " "))
              (string "`make " target "` runs the runner without the wide "
                      "policy: " batch))
      (assert (not (string/contains? batch "--timeout"))
              (string "`make " target "` widens every file in the batch with "
                      "one `--timeout`: " batch)))))

# A family that matches nothing is an error nowhere: the runner never widens
# it, and the file it was written for goes back to the default budget.
(eprintln "scenario: every family named for the wider budget matches a suite file")
(each family families
  (assert (any? (fn [p] (string/contains? p family)) suite-files)
          (string "the Makefile widens `" family
                  "`, which matches no suite file")))

# The counter-factual: parse the flags and never consult them where the
# deadline is chosen. Only the runner can say what a path's budget is.
(eprintln "scenario: the runner widens the named families and nothing else")
(assert (> wide-ms default-ms)
        (string "the wide budget " wide-ms " ms is not wider than the default "
                default-ms " ms"))
(let [controls ["tests/lang/arithmetic.lisp" "tests/lang/strings.lisp"]]
  (each c controls
    (assert (file-exists? c) (string c " is gone")))
  (each [path ms] (budgets wide-flags [;controls ;(filter wide? suite-files)])
    (assert (= ms (if (wide? path) wide-ms default-ms))
            (string "the runner gives " path " " ms " ms"))))

# The dashboards loop a shape under a heap gauge until each interval converges,
# and run in the implementation suite like every other file, so the wide
# budget is the only thing between them and the default.
(eprintln "scenario: the dashboards run under the wide budget")
(each [path ms] (budgets wide-flags
                         ["tests/impl/oracle.lisp" "tests/impl/plumb.lisp"])
  (assert (= ms wide-ms) (string path " is a dashboard, and got " ms " ms")))

# spec.lisp checks every fence of the spec documents on each tier, about 40 s of
# CPU on the VM tier of a CI runner. The counter-factual: leave it at the
# default, and a loaded batch kills it with no verdict recorded.
(eprintln "scenario: the spec checks run under the wide budget")
(each [path ms] (budgets wide-flags ["tests/lang/spec.lisp"])
  (assert (= ms wide-ms)
          (string path
                  " checks every spec fence and takes the wide budget, got " ms
                  " ms")))

# The counter-factual for this file: h2-stream-share.lisp gave itself 120 s and
# ran under the runner's 60 s default, so a slow box killed it mid-stream.
(eprintln "scenario: no suite file outlives the runner budget before its own deadline")
(let [declaring (filter (fn [p] (not (nil? (deadline-of p)))) suite-files)]
  (assert (not (empty? declaring))
          (string "no suite file declares a deadline; teach this file the new "
                  "shape rather than letting it pass by matching nothing"))
  (each [path ms] (budgets wide-flags declaring)
    (assert (> ms (* 1000 (deadline-of path)))
            (string path " gives itself " (deadline-of path)
                    " s and the runner " "kills its form at " ms
                    " ms, so its own diagnostic never " "prints"))))

# Every check above reads `--budget`, which answers from the flags alone. A
# runner that chose the budget and bound it nowhere would pass them all, so a
# form has to run. The fixture sleeps rather than hangs, and the second arm
# withholds `--wide`, so a pass is not merely a short sleep.
(eprintln "scenario: a wide path runs a form the default budget would kill")
(with-temp-dir dir
  # The pattern is matched against the path, so naming the file makes it wide.
  (let [fixture (write-fixture dir "wide-budget-fixture.lisp" "(ev/sleep 3)\n")
        limits ["--timeout" "1000" "--wide-timeout" "15000"]
        wide (elle-test (path/join dir "wide.db")
                        [;limits "--wide" "wide-budget-fixture.lisp" fixture])
        narrow (elle-test (path/join dir "narrow.db") [;limits fixture])]
    (assert (= wide:exit 0)
            (string "a form of a --wide path runs to its end under "
                    "--wide-timeout: " wide:out wide:err))
    (assert (not (= narrow:exit 0))
            (string "the same fixture, not named --wide, is killed at 1000 ms: "
                    narrow:err))
    (assert (string/contains? (string narrow:out narrow:err) "timeout")
            (string "the unnamed fixture gates as a timeout: " narrow:err))))

(eprintln "all budget scenarios passed")
