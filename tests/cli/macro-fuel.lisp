(elle/epoch 18)
## audited: 2026-11-22
## A macro that never returns fails the compile that expands it: an expansion spends a budget apart from fiber fuel.
## tests/cli/overview.md
## docs/macros.md
## docs/runtime.md
##
## Each scenario starts the binary under a deadline, because the failure each
## one guards is a hang. A transformer that spends the whole expansion budget
## returns in well under a second on a release build, and the rest of the
## deadline is slack for a debug build on a loaded machine.

(include-file "../runner/lib/harness.lisp")

(def deadline 120)

(def spin "(defmacro spin [] (forever nil))")

(defn run-file [dir name source]
  "Write SOURCE as NAME under DIR, run it as a file, and answer what it left."
  (elle-feed [(write-fixture dir name source)] "" :cwd dir :deadline deadline))

(defn run-stdin [dir source]
  "Run SOURCE from the standard input, and answer what it left."
  (elle-feed ["-"] source :cwd dir :deadline deadline))

(defn names-budget [text what]
  "Require TEXT to name WHAT and the budget it ran out of."
  (assert (and (string/contains? text what) (string/contains? text "fuel"))
          (string "the error names " what " and its budget: " text)))

(defn caught [form]
  "A program that prints the message of FORM's error, or what FORM answered."
  (string "(let [[ok? err] (protect " form ")]\n"
          "  (print (if ok? \"answered\" (get err :message))))\n"))

(with-temp-dir dir
  (eprintln "scenario: a looping macro fails the compile of a file")
  (let [r (run-file dir "spin.lisp" (string spin "\n(spin)\n"))]
    (assert (= r:exit 1)
            (string "the compile fails, and no hang reaches the deadline: "
                    r:exit "; " r:err))
    (names-budget r:err "macro 'spin'"))

  (eprintln "scenario: a looping macro fails eval")
  (let [r (run-stdin dir (caught (string "(eval '(begin " spin " (spin)))")))]
    (assert (= r:exit 0) (string "eval raises, and protect catches it: " r:err))
    (names-budget r:out "macro 'spin'"))

  (eprintln "scenario: a looping macro fails compile/analyze of its file")
  (let [r (run-stdin dir
                     (caught (string "(compile/analyze \"" spin " (spin)\")")))]
    (assert (= r:exit 0) (string "the analysis raises: " r:err))
    (names-budget r:out "macro 'spin'"))

  # The counter-factual is the analysis that compiled each literal import,
  # which expanded the module's `spin` and hung until the deadline.
  (eprintln "scenario: analyzing an importer expands no macro of its import")
  (let [module (write-fixture dir "m.lisp"
                              (string "(begin " spin " (fn [] {:x (spin)}))\n"))
        r (run-stdin dir
                     (string "(compile/analyze \"(def m ((import/load-file \\\""
                             module "\\\"))) m\")\n(print \"analyzed\")\n"))]
    (assert (= r:exit 0) (string "the analysis returns: " r:exit "; " r:err))
    (assert (= (string/trim r:out) "analyzed") (string "printed " r:out)))

  (eprintln "scenario: a looping begin-for-syntax fails the compile")
  (let [r (run-stdin dir
                     (caught "(eval '(begin (begin-for-syntax (def x (forever nil))) 1))"))]
    (assert (= r:exit 0) (string "eval raises, and protect catches it: " r:err))
    (names-budget r:out "begin-for-syntax"))

  # A fiber whose fuel runs out halfway through the transformer still gets the
  # expansion, because the transformer neither spends that fuel nor can pause
  # when it runs out. The counter-factual runs the transformer on the fiber's
  # fuel, which runs out inside it, so the expansion fails on `:fuel`.
  (eprintln "scenario: a transformer does not spend the fiber's fuel")
  (let [r (run-stdin dir
                     (string "(def f (fiber/new (fn [] (eval '(begin (defmacro count-up [] "
                             "(begin (var i 0) (while (< i 5000) (assign i (+ i 1))) i)) "
                             "(count-up))))\n  |:fuel :error|))\n"
                             "(fiber/set-fuel f 1000)\n(print (fiber/resume f))\n"))]
    (assert (= r:exit 0) (string "the program runs: " r:err))
    (assert (= (string/trim r:out) "5000")
            (string "the expansion answers its count: " r:out)))

  (eprintln "scenario: a transformer that loops a hundred thousand times expands")
  (let [r (run-stdin dir
                     (string "(defmacro count-up [] (begin (var i 0) "
                             "(while (< i 100000) (assign i (+ i 1))) i))\n"
                             "(print (count-up))\n"))]
    (assert (= r:exit 0) (string "the program runs: " r:err))
    (assert (= (string/trim r:out) "100000") (string "printed " r:out)))

  # `protect` runs its body on a fiber of its own. The loop is not silent,
  # because its strict destructuring may raise, and it makes no call, so it
  # spends the budget fast. The counter-factual is a budget only the
  # expansion's own fiber spends: the child holds none, so the loop runs until
  # the deadline.
  (eprintln "scenario: a loop on a child fiber fails the compile")
  (let [r (run-file dir "child.lisp"
                    (string "(defmacro on-child []\n"
                            "  (begin (protect (begin (var pair [1]) "
                            "(while true (let [[one] pair] one)))) :caught))\n"
                            "(print (on-child))\n"))]
    (assert (= r:exit 1) (string "the compile fails: " r:exit "; " r:err))
    (names-budget r:err "macro 'on-child'"))

  # `helper` holds no call, so its signal is silent. The counter-factual is the
  # silent spend outside an expansion, which never stops the fiber, so the loop
  # runs until the deadline.
  (eprintln "scenario: a silent loop a transformer calls fails the compile")
  (let [r (run-file dir "silent.lisp"
                    (string "(begin-for-syntax (def helper (fn [go] (while go nil) 0)))\n"
                            "(defmacro spin-in-helper [] (helper true))\n"
                            "(print (spin-in-helper))\n"))]
    (assert (= r:exit 1) (string "the compile fails: " r:exit "; " r:err))
    (names-budget r:err "macro 'spin-in-helper'")))

(eprintln "all macro-fuel scenarios passed")
