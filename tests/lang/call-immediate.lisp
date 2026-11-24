(elle/epoch 18)
# audited: 2026-11-22
# A call on an immediate the compiler knows does not compile, and a collection callee still runs.
#
# docs/functions.md
#
# Counter-factual: each refused form compiled, and failed only when its call
# ran, with "Cannot call". A form inside `(fn [] ...)` never runs its call, so
# a refusal that waits for the run would let every one of these compile.

(defn compiles? [form]
  "Whether eval compiles form. A fn form compiles without running its body."
  (first (protect (eval form))))

(defn refusal [form]
  "The message eval reports for form, which must not compile."
  (def [ok? err] (protect (eval form)))
  (assert (not ok?) "the form must not compile")
  (get err :message))

# ── A literal immediate callee ───────────────────────────────────────

(assert (not (compiles? '(fn [] (nil 3)))) "nil is not callable")
(assert (not (compiles? '(fn [] (nil)))) "nil with no argument")
(assert (not (compiles? '(fn [] (() 3)))) "the empty list is not callable")
(assert (not (compiles? '(fn [] (true 3)))) "true is not callable")
(assert (not (compiles? '(fn [] (false 3)))) "false is not callable")
(assert (not (compiles? '(fn [] (0 3)))) "an integer is not callable")
(assert (not (compiles? '(fn [] (1.5 3)))) "a float is not callable")
(assert (not (compiles? '(fn [] (:k 3)))) "a keyword is not callable")
(assert (not (compiles? '(fn [] (':k 3)))) "a quoted keyword is not callable")
(assert (not (compiles? '(fn [] ('5 3)))) "a quoted number is not callable")
(assert (not (compiles? '(fn [] ('sym 3)))) "a quoted symbol is not callable")
(assert (not (compiles? '(fn [xs] (:k ;xs))))
        "a splice does not hide the callee")
(assert (not (compiles? '(fn [m] (+ 1 (:k m)))))
        "nor does a call out of tail position")

(def keyword-message (refusal '(fn [] (:k 3))))
(assert (string/contains? keyword-message "not callable")
        "the error says the callee is not callable")
(assert (string/contains? keyword-message ":k") "the error spells the callee")

# ── An immutable binding to an immediate ─────────────────────────────

(assert (not (compiles? '(let [k :k]
                           (fn [] (k 3)))))
        "a let binding to a keyword, called in a nested function")
(assert (not (compiles? '(fn []
                           (let [n nil]
                             (n 3))))) "a let binding to nil")
(assert (not (compiles? '(fn []
                           (def k 5)
                           (k 3)))) "a def in a body")
(assert (not (compiles? '(fn []
                           (letrec [k :k]
                             (k 3))))) "a letrec binding")
(assert (not (compiles? '(fn []
                           (let [a :k
                                 b a]
                             (b 3))))) "a binding to a binding to a keyword")

# The analyzer meets this call before the initializer of k, so a check made
# at the call would let it compile.
(assert (not (compiles? '(fn []
                           (letrec [f (fn [] (k 3))
                                    k :k]
                             (f)))))
        "a letrec initializer calls a binding a later one sets")

(def [file-ok? file-err]
  (protect (compile/whole-module "(def k :k)\n(defn f [s] (k s))" "<file>")))
(assert (not file-ok?) "a file's def to a keyword, called from a later function")
(assert (string/contains? (get file-err :message) "not callable")
        "the file reports the callee is not callable")

# ── A callee checked when the call runs ──────────────────────────────

(assert (compiles? '(let [@k :k]
                      (fn [] (k 3)))) "a mutable binding to a keyword compiles")
(assert (compiles? '(fn []
                      (def k (keyword "k"))
                      (k 3))) "a binding to a computed keyword compiles")
(assert (compiles? '(fn [] ('(1 2) 0)))
        "a quoted list is not an immediate, so it compiles")

(def computed (keyword "k"))
(def [computed-ok? computed-err] (protect (computed 3)))
(assert (not computed-ok?) "a computed keyword fails when the call runs")
(assert (= :type-error (get computed-err :error))
        "the run-time failure is a type error")

(def [list-ok? _] (protect ('(1 2) 0)))
(assert (not list-ok?) "a quoted list fails when the call runs")

# ── A collection callee ──────────────────────────────────────────────

(assert (= 1 ({:a 1} :a)) "a struct reads a field")
(assert (= 1 (@{:a 1} :a)) "a mutable struct reads a field")
(assert (= 20 ([10 20] 1)) "an array reads an element")
(assert (= 10 (@[10 20] 0)) "a mutable array reads an element")
(assert (= "b" ("abc" 1)) "a string reads a grapheme")
(assert (= "b" (@"abc" 1)) "a mutable string reads a grapheme")
(assert (= 1 (b[1 2] 0)) "bytes read a byte")
(assert (= true (|1 2| 1)) "a set tests membership")
(assert (= false (@|1 2| 3)) "a mutable set tests membership")

(def fields {:a 1})
(assert (= 1 (fields :a)) "an immutable binding to a struct")
(defn read-field [k]
  (fields k))
(assert (= 1 (read-field :a)) "a function calls a file binding to a struct")

# ── A binding that only looks constant ───────────────────────────────

# A def of a name makes a new binding, so the second def's struct is the
# callee, not the first def's keyword.
(def shadowed :k)
(def shadowed {:k 1})
(assert (= 1 (shadowed :k)) "a later def of the name holds a struct")

# A destructuring letrec holds each name before it holds its function.
(assert (= 3
           (letrec [[f g] [(fn [] 1) (fn [] 2)]]
             (+ (f) (g)))) "a destructured letrec binding calls its function")

(def [first-fn second-fn] [(fn [] 1) (fn [] 2)])
(assert (= 3 (+ (first-fn) (second-fn))) "a destructured def calls its function")
