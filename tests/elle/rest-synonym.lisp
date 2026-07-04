(elle/epoch 12)
# `&rest` is a synonym for `&` in EVERY rest-collector position: function
# parameter lists, destructuring patterns (list, array, struct), match
# patterns, and defmacro parameter lists (docs/functions.md § Variadic
# functions). Recognition goes through one shared predicate
# (`syntax::is_rest_marker`), so the two spellings cannot drift.

# ── Function parameters ──────────────────────────────────────────────
(defn sum-amp [& nums]
  (fold + 0 nums))
(defn sum-rest [&rest nums]
  (fold + 0 nums))
(assert (= (sum-rest 1 2 3) 6) "fn params: &rest collects into a list")
(assert (= (sum-amp 1 2 3) (sum-rest 1 2 3)) "fn params: &rest agrees with &")
(assert (= (sum-rest) 0) "fn params: &rest with no extra args is empty")

(assert (= ((fn [a &rest r] (length r)) 1 2 3) 2)
        "anonymous fn: &rest after fixed params")

# &opt composes with &rest exactly as with &.
(defn opt-rest [a &opt b &rest r]
  [a b (length r)])
(assert (= (opt-rest 1) [1 nil 0]) "&opt then &rest: defaults")
(assert (= (opt-rest 1 2 3 4) [1 2 2]) "&opt then &rest: collects")

# ── Destructuring patterns ───────────────────────────────────────────
(def (head &rest tail) (list 1 2 3 4))
(assert (= head 1) "list destructure: head")
(assert (= tail (list 2 3 4)) "list destructure: &rest tail is a list")

(def [first-el &rest more] [10 20 30])
(assert (= first-el 10) "array destructure: first")
(assert (array? more) "array destructure: &rest collects into an array")
(assert (= (get more 0) 20) "array destructure: &rest contents")

(def {:a a-val &rest others} {:a 1 :b 2 :c 3})
(assert (= a-val 1) "struct destructure: named field")
(assert (= (length others) 2) "struct destructure: &rest collects the rest")

# ── match patterns ───────────────────────────────────────────────────
(assert (= (match (list 1 2 3)
             (a &rest r) (length r)
             _ :no-match) 2) "match list pattern: &rest")
(assert (= (match [1 2 3]
             [a &rest r] (length r)
             _ :no-match) 2) "match array pattern: &rest")

# ── defmacro parameters ──────────────────────────────────────────────
(defmacro count-args (first &rest rest)
  (length rest))
(assert (= (count-args a b c d) 3) "defmacro params: &rest collects")

(println "rest-synonym: OK")
