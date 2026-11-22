(elle/epoch 18)
## audited: 2026-11-22
## An old program and the old module of several forms it imports run unchanged, before and after elle rewrite.
## tests/cli/overview.md
## docs/epochs.md
## docs/modules.md
##
## The pair is the shape every program written before epoch 16 has: a module
## file of several forms beside a program that imports it by a bare spec, run
## from their directory.

(include-file "../runner/lib/harness.lisp")
(include-file "lib/modules.lisp")

(def module
  (string "(elle/epoch 12)\n(def greeting \"Hi\")\n"
          "(defn hi [n] (string greeting \" \" n))\n(fn [] {:hi hi})\n"))
(def program
  (string "(elle/epoch 12)\n(def m ((import \"old\")))\n"
          "(print (m:hi \"x\"))\n"))

(with-temp-dir dir
  (defn old-pair [name]
    "A directory holding the old module and the old program."
    (let [root (path/join dir name)]
      (file/mkdir-all root)
      (file/write (path/join root "old.lisp") module)
      (file/write (path/join root "main.lisp") program)
      root))

  (defn runs-the-program [root what]
    (let [r (run-in root ["main.lisp"] "")]
      (assert (= r:exit 0) (string what "; stderr: " r:err))
      (assert (= (string/trim r:out) "Hi x") (string what ": printed " r:out))))

  # The counter-factual refused the module: it holds 3 forms, and a module
  # holds one.
  (eprintln "scenario: an old program imports an old module of several forms")
  (runs-the-program (old-pair "before") "the old program runs")

  (eprintln "scenario: the rewritten module is one form and still loads")
  (let [root (old-pair "after")
        r (run-in root ["rewrite" "old.lisp" "main.lisp"] "")]
    (assert (= r:exit 0) (string "the rewrite succeeds; stderr: " r:err))
    (let [forms (run-in root ["-"]
                        (string "(print (length (compile/read-forms "
                                "(file/read \"old.lisp\") \"old.lisp\")))"))]
      (assert (= forms:exit 0) (string "the module reads; stderr: " forms:err))
      (assert (= (string/trim forms:out) "1")
              (string "the rewritten module holds one form, got " forms:out)))
    (runs-the-program root "the program runs against the rewritten module")))

(eprintln "all module-old scenarios passed")
