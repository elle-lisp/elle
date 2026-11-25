(elle/epoch 18)
## audited: 2026-11-22
## The REPL fed through a pipe: one form at a time, what a line binds, and the errors a session survives.
## tests/cli/overview.md
## docs/pipeline.md

(include-file "../runner/lib/harness.lisp")
(include-file "lib/repl.lisp")

(eprintln "scenario: a line evaluates and echoes its value")
(echoes "(+ 1 2)\n" ["3"] "a single form")

(eprintln "scenario: several forms on one line each echo and each run")
(echoes "(+ 1 2) (+ 3 4)\n" ["3" "7"] "two forms on one line")
(let [r (session "(print 1) (print 2) (print 3)\n")]
  (assert (= r:exit 0) (string "three prints; stderr: " r:err))
  (assert (string/contains? r:out "123")
          (string "each form's side effect runs, in order: " r:out)))

(eprintln "scenario: a def persists across lines")
(echoes "(def x 10)\n(+ x 1)\n" ["10" "11"] "a def and its use")
(echoes "(def add (fn (a b) (+ a b)))\n(add 3 4)\n" ["7"] "a function def")
(echoes "(defn double (x) (* x 2))\n(double 5)\n" ["10"] "a defn")
(echoes "(def x 10)\n(def y 20)\n(+ x y)\n" ["30"] "two defs")

(eprintln "scenario: a redefinition is the value later lines read")
(let [r (echoes "(def x 1)\n(def x 2)\nx\n" ["1" "2"] "two defs of one name")
      results (filter (fn [l] (string/contains? l "⟹")) (lines r:out))]
  (assert (= (length results) 3) (string "three results: " r:out))
  (assert (string/ends-with? (string/trim (get results 2)) "⟹ 2")
          (string "the last line reads the redefined value: " r:out)))

# The counter-factual recorded each line's functions for later lines to splice
# into a fused `map`, keeping the first definition, so the map answered [1 1]
# while a direct call answered 2.
(eprintln "scenario: a redefined function is the body a later map runs")
(echoes "(defn f [x] 1)\n(defn f [x] 2)\n(map f [1 2])\n(f 0)\n" ["[2 2]" "2"]
        "a map after a redefinition")

(eprintln "scenario: an expression across lines accumulates until it closes")
(echoes "(+ 1\n   2)\n" ["3"] "a call across two lines")
(echoes "(let [x 10]\n  (+ x 1))\n" ["11"] "a let across two lines")
(echoes (string "(defn fib (n)\n  (if (< n 2) n\n    (+ (fib (- n 1)) "
                "(fib (- n 2)))))\n(fib 10)\n") ["55"]
        "a defn across three lines")

(eprintln "scenario: a var and a destructuring def persist across lines")
(echoes "(var x 10)\n(+ x 1)\n" ["11"] "a var")
(echoes "(def [a b] [1 2])\n(+ a b)\n" ["3"] "an array destructuring def")
(echoes "(def {:x a :y b} {:x 10 :y 20})\n(+ a b)\n" ["30"]
        "a struct destructuring def")
(echoes "(var [x y] [1 2])\n(+ x y)\n" ["3"] "a destructuring var")

(eprintln "scenario: a macro persists across lines")
(echoes "(defmacro double (x) (list (quote *) x 2))\n(double 5)\n" ["10"]
        "a defmacro")
(echoes (string "(defmacro m (x) (list (quote +) x 1))\n(m 10)\n"
                "(defmacro m (x) (list (quote +) x 2))\n(m 10)\n") ["11" "12"]
        "a redefined macro")
(echoes (string "(def offset 100)\n"
                "(defmacro add-offset (x) (list (quote +) x (quote offset)))\n"
                "(add-offset 5)\n") ["105"] "a macro that reads a def")

(eprintln "scenario: an error ends the session's status, and not the session")
(let [r (session "(+ 1\n")]
  (assert (= r:exit 1) (string "unterminated input exits 1, got " r:exit))
  (assert (string/contains? r:err "unterminated") (string "stderr: " r:err)))
(let [r (session "(/ 1 0)\n")]
  (assert (= r:exit 1) (string "a runtime error exits 1, got " r:exit)))
# An undefined name does not prevent the valid input after it.
(let [r (session "bad-symbol\n(+ 1 2)\n")]
  (assert (string/contains? r:err "undefined") (string "stderr: " r:err))
  (assert (echoed? r "3") (string "the session recovers: " r:out)))

(eprintln "all repl scenarios passed")
