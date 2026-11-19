(elle/epoch 18)
# audited: 2026-11-18
# A def binds its name from its own form to the end of its body; a read before it resolves outside the body.
# docs/bindings.md
# docs/epochs.md
#
# The subjects are strings, so each compiles as a file of its own, at the epoch
# it declares, and one that fails cannot fail this file. The counter-factual:
# a body used to bind every name its defs bind before any form ran. A function
# could call one defined after it, and a direct read in a def's value read the
# binding before it. Each subject that says otherwise compiles, or answers
# differently, on that compiler.

(defn run [src]
  "Compile SRC as a file, run it, and answer its value."
  ((get (get (compile/whole-module src "<p>") 0) 1)))

(defn compile-error [src]
  "The message of the compile error SRC raises."
  (let [[ok? err] (protect (compile/whole-module src "<p>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

(defn refused [src message]
  "Assert that SRC fails to compile with an error that holds MESSAGE."
  (let [got (compile-error src)]
    (assert (string/contains? got message) (string "expected " message ": " got))))

# ── A read before the def ─────────────────────────────────────────────

(refused "(defn f [] (g))\n(defn g [] 1)\n(f)" "undefined variable: g")
(refused "(defn outer []\n  (defn f [] (g))\n  (defn g [] 1)\n  (f))"
         "undefined variable: g")
(refused "(defn outer []\n  (def a b)\n  (def b 7)\n  a)"
         "undefined variable: b")

(assert (= (run "(def before (abs -3))\n(defn abs [x] :mine)\n[before (abs -3)]")
           [3 :mine]) "a read before the file's def finds the primitive")
(assert (= (run "(defn f [] (abs -1))\n(defn abs [x] :mine)\n[(f) (abs -1)]")
           [1 :mine])
        "a function written before the file's def calls the primitive")

(assert (= (run "(def greeting :outer)
(defn early []
  (defn say [] greeting)
  (def before (say))
  (def greeting :inner)
  [before (say) greeting])
(early)")
           [:outer :outer :inner])
        "a function written before the body's def reads the binding outside the body")

(assert (= (run "(def level 1)
(defn deeper []
  (def before level)
  (def level 5)
  [before level])
(deeper)")
           [1 5])
        "a direct read before the body's def reads the binding outside the body")

# ── The def's own value ───────────────────────────────────────────────

(assert (= (run "(defn countdown [n] (if (= n 0) :done (countdown (- n 1))))\n(countdown 3)")
           :done) "a lambda in a def's value reads the def's binding")
(assert (= (run "(defn outer []
  (def climb (fn [n] :replaced))
  (def climb (fn [n] (if (> n 30) n (climb (+ n 10)))))
  (climb 1))
(outer)")
           31) "a lambda in a redefinition's value reads the redefinition")

(refused "(defn f []\n  (def x 1)\n  (def x (+ x 1))\n  x)"
         "'x' referenced before its initialization")
(refused "(def x 1)\n(defn f []\n  (def x (+ x 1))\n  x)"
         "'x' referenced before its initialization")
(refused "(def n 1)\n(def n (+ n 1))\nn"
         "'n' referenced before its initialization")

# ── Each def is a binding of its own ──────────────────────────────────

(assert (= (run "(defn redefine []
  (def x 3)
  (def p (fn [] (= x 3)))
  (def x 4)
  [x (p)])
(redefine)")
           [4 true]) "a function keeps the binding it saw where it was written")
(assert (= (run "(def again 1)
(defn read-first [] again)
(def again 2)
(defn read-second [] again)
[(read-first) (read-second)]")
           [1 2]) "the file's defs of one name are bindings of their own")
(refused "(defn f []\n  (def x 1)\n  (def @x 2)\n  x)"
         "both mutable and immutable")
(refused "(def @v @[1])\n(def v 2)\nv" "both mutable and immutable")

# ── Functions that call each other ────────────────────────────────────

(assert (= (run "(defn parity [n]
  (letrec [ev? (fn [k] (if (= k 0) true (od? (- k 1))))
           od? (fn [k] (if (= k 0) false (ev? (- k 1))))]
    (ev? n)))
[(parity 10) (parity 7)]")
           [true false]) "functions that call each other bind in one letrec")

# ── A file that declares an older epoch ───────────────────────────────

(assert (= (run "(elle/epoch 17)\n(defn f [] (g))\n(defn g [] 1)\n(f)") 1)
        "epoch 17 moves a function the file's function calls above it")
(assert (= (run "(elle/epoch 17)\n(defn f [] (g))\n(def k 2)\n(defn g [] k)\n(f)")
           2) "epoch 17 moves a literal def with the function that reads it")
(refused "(elle/epoch 17)\n(defn f [] (g))\n(+ 1 2)\n(defn g [] 1)\n(f)"
         "'g' is read before its def")
(assert (= (run "(elle/epoch 17)
(defn outer []
  (defn ping [n] (if (= n 0) :done (pong (- n 1))))
  (defn pong [n] (ping n))
  (ping 4))
(outer)")
           :done) "epoch 17 reads a body's mutually recursive defs as a letrec")
(assert (= (run "(elle/epoch 17)\n(def x 1)\n(def x (+ x 1))\nx") 2)
        "epoch 17 reads a def whose value reads its own name as a let")
(assert (= (run "(elle/epoch 17)\n(defn f [] (abs -1))\n(defn abs [x] :mine)\n(f)")
           :mine)
        "epoch 17 keeps a forward read of a primitive's name on the file's def")

(let [message (compile-error "(elle/epoch 17)\n(defn f [] n)\n(println 1)\n(def n 1)\n(f)")]
  (assert (string/contains? message "'n' is read before its def")
          (string "a def does not move past an expression, a letrec cannot "
                  "hold the expression, and the read is refused: " message))
  (assert (string/contains? message "<p>:2:12")
          (string "the message names the read: " message)))
(refused "(elle/epoch 17)\n(defn f [] (abs -1))\n(println 1)\n(defn abs [x] :mine)"
         "'abs' is read before its def")
# Above the def of `y`, `abs` would turn the primitive call into a call of
# its own.
(refused "(elle/epoch 17)\n(defn f [] (abs -1))\n(def y (abs -2))\n(defn abs [x] :mine)\n(f)"
         "'abs' is read before its def")
