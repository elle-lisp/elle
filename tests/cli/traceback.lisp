(elle/epoch 18)
## audited: 2026-11-23
## An uncaught error is reported at the form that raised it: past a handled error, through a defer, and from an unjoined fiber.
## tests/cli/overview.md
## docs/impl/vm.md
## docs/impl/graft.md
##
## Each fixture carries one marker keyword per raising form, so a scenario
## names a form by what it raises rather than by a line number that an edit to
## the fixture would move. The program defines each raising function, so the
## graft splices it into its caller, and the report still names its own line.

(include-file "../runner/lib/harness.lisp")

(defn line-of [source marker]
  "The line, numbered from one, of the one line of SOURCE that holds MARKER."
  (let [hits (filter (fn [i]
                       (string/contains? (get (string/split source "\n") i)
                         marker)) (range (length (string/split source "\n"))))]
    (assert (= (length hits) 1)
            (string marker " must sit on exactly one line, found " hits))
    (+ (first hits) 1)))

(defn reported [err path]
  "The `at PATH:LINE:COL` line of ERR, or nil when the report names no place
   in PATH."
  (find (fn [l]
          (and (string/starts-with? (string/trim l) "at ")
               (string/contains? l (string path ":")))) (lines err)))

(defn dies [path]
  "Run PATH, which must die on an uncaught error, and answer what it left."
  (let [r (elle-run [path])]
    (assert (not (= r:exit 0))
            (string path " must die on an uncaught error; stderr: " r:err))
    r))

(defn at? [line path n]
  "Whether the report LINE names line N of PATH."
  (and (not (nil? line)) (string/contains? line (string path ":" n ":"))))

(def plain
  (string "(defn uncaught-raise []\n"
          "  (error {:error :only :message \"reaches the root\"}))\n"
          "(uncaught-raise)\n"))

(def after-catch
  (string "(defn caught-raise []\n"
          "  (error {:error :first :message \"caught and handled\"}))\n"
          "(defn uncaught-raise []\n"
          "  (error {:error :second :message \"reaches the root\"}))\n"
          "(protect (caught-raise))\n" "(uncaught-raise)\n"))

(def through-defer
  (string "(defn caught-raise []\n"
          "  (error {:error :first :message \"caught and handled\"}))\n"
          "(defn uncaught-raise []\n"
          "  (error {:error :second :message \"reaches the root through defer\"}))\n"
          "(protect (caught-raise))\n" "(defer\n"
          "  (println \"cleanup ran\")\n" "  (uncaught-raise))\n"))

(def unjoined
  (string "(defn uncaught-raise []\n"
          "  (error {:error :orphan :message \"raised in an unjoined fiber\"}))\n"
          "(ev/spawn (fn [] (uncaught-raise)))\n" "(ev/sleep 0.05)\n"))

(with-temp-dir dir
  (eprintln "scenario: an uncaught error names the form that raised it")
  (let [path (write-fixture dir "plain.lisp" plain)
        r (dies path)
        line (reported r:err path)]
    (assert (at? line path (line-of plain ":only"))
            (string "the report must name line " (line-of plain ":only")
                    "; stderr: " r:err)))

  # The trap: the recorded location is first-writer-wins, so that the raising
  # frame beats the frames it unwinds through. Nothing else clears it, so a
  # location that outlives its own error is reported for every error after it.
  # The counter-factual: assert only that some location is printed, and this
  # passes while naming a form from an unrelated error.
  (eprintln "scenario: a handled error lends its location to no later one")
  (let [path (write-fixture dir "after-catch.lisp" after-catch)
        r (dies path)
        line (reported r:err path)]
    (assert (not (at? line path (line-of after-catch ":first")))
            (string "the handled error's line was reported; stderr: " r:err))
    (assert (at? line path (line-of after-catch ":second"))
            (string "the report must name line " (line-of after-catch ":second")
                    "; stderr: " r:err)))

  # The trap: `defer` catches with a fiber mask, runs its cleanup, then
  # re-raises. The catch ends the first propagation, so a location that lived
  # only in the VM would be gone by the re-raise, and the `(defer …)` form
  # would take the blame.
  (eprintln "scenario: an error defer re-raises names the form that raised it")
  (let [path (write-fixture dir "through-defer.lisp" through-defer)
        r (dies path)
        line (reported r:err path)]
    (assert (string/contains? r:out "cleanup ran")
            (string "the cleanup must run before the error reaches the root: "
                    r:out))
    (assert (not (at? line path (line-of through-defer ":first")))
            (string "the handled error's line was reported; stderr: " r:err))
    (assert (at? line path (line-of through-defer ":second"))
            (string "the report must name line "
                    (line-of through-defer ":second") "; stderr: " r:err)))

  # The trap: the scheduler catches a spawned fiber's error, then surfaces it
  # long after the raising frame is gone. The counter-factual: surface it as
  # `(error (fiber/value f))`, and every orphan's error is reported at that one
  # stdlib line.
  (eprintln "scenario: an unjoined fiber's error names the form that raised it")
  (let [path (write-fixture dir "unjoined.lisp" unjoined)
        r (dies path)
        line (reported r:err path)]
    (assert (at? line path (line-of unjoined ":orphan"))
            (string "the report must name line " (line-of unjoined ":orphan")
                    "; stderr: " r:err))))

(eprintln "all traceback scenarios passed")
