(elle/epoch 15)
# audited: 2026-10-22
# What one compile expands is a unit, and the names gensym answers, the begin-for-syntax definitions and the scopes are the unit's own.
# docs/macros.md

# ── gensym ───────────────────────────────────────────────────────────

# While a unit expands, gensym names its symbols after the unit, and the datum
# one eval compiles is a unit. The counter-factual is a count that runs across
# the process: the second eval of one datum answers a name the first did not.
(def one-unit
  '(begin
     (defmacro fresh []
       (list 'quote (gensym "t")))
     (fresh)))
(def other-unit
  '(begin
     (defmacro other []
       (list 'quote (gensym "t")))
     (other)))

(assert (= (eval one-unit) (eval one-unit)) "one unit expands to one name")
(assert (not (= (eval one-unit) (eval other-unit)))
        "two units expand to two names")

# ── begin-for-syntax ─────────────────────────────────────────────────

# A begin-for-syntax definition belongs to the unit that makes it, and each
# eval is a unit. The counter-factual is an eval expander that keeps the
# definitions: the third eval below finds the first one's helper and answers 42.
(eval '(begin-for-syntax (def eval-helper (fn [] 42))))
(eval '(defmacro uses-eval-helper []
        (eval-helper)))
(let [[ok? _] (protect (eval '(uses-eval-helper)))]
  (assert (not ok?)
          "an eval does not see an earlier eval's begin-for-syntax definition"))

# ── A macro another unit wrote ───────────────────────────────────────

## `gen` writes `m` whole in one eval, and `m`'s transformer compiles in a later
## one, where `wrap` binds `+` around the body `gen` wrote. That `+` is `gen`'s,
## not `wrap`'s. The counter-factual is a flip that leaves an expansion's scope
## on its arguments: `wrap`'s `+` then captures the body's, and `m` answers
## (- 2 1).
(eval '(defmacro wrap [body]
        (list 'let ['+ '-] body)))
(eval '(begin
         (defmacro gen []
           (list 'defmacro 'm [] (list 'wrap '(+ 2 1))))
         (gen)))
(assert (= (eval '(m)) 3)
        "a binder one unit's expansion makes does not capture another unit's name")
