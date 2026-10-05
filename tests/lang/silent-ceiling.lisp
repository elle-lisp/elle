(elle/epoch 15)
# audited: 2026-10-05
# ── silent!: compile-time enforcement ─────────────────────────────────
#
# (silent!) is (attune! ||), the empty ceiling: the body's inferred signal
# must be empty, :error included, and any excess bit is a compile error.
# docs/signals/inference.md

# ── Should compile: silent! with pure control flow ───────────────────

(defn select [flag a b]
  (silent!)
  (if flag a b))

(assert (= (select true 1 2) 1) "select true")
(assert (= (select false 1 2) 2) "select false")

(defn always [x]
  (silent!)
  x)

(assert (= (always 42) 42) "always returns its argument")

# ── Compile-time rejection: arithmetic emits {:error} ────────────────
#
# The same body without the ceiling compiles, so the refusal is the
# ceiling's.

(defn compiles? [src]
  "Whether src compiles. Nothing in it runs, so a refusal is the compiler's."
  (first (protect (compile/whole-module src "<ceiling>"))))

(assert (compiles? "(defn add [x y] (+ x y))") "the body alone compiles")
(assert (not (compiles? "(defn bad-add [x y] (silent!) (+ x y))"))
        "silent! rejects arithmetic at compile time")

# The same ceiling spelled as attune! accepts and refuses the same bodies:
# one concept, one check. Counter-factual: silent! kept a check of its own.
(assert (not (compiles? "(defn bad-add [x y] (attune! ||) (+ x y))"))
        "(attune! ||) rejects arithmetic at compile time")
(assert (compiles? "(defn always [x] (attune! ||) x)")
        "(attune! ||) accepts what silent! accepts")

# A ceiling built as data reads the same: a quoted set literal reaches eval
# as a list, and the analyzer takes it as the set it spells.
(def [quoted-ok? _]
  (protect (eval '(defn bad-add [x y]
                   (attune! ||)
                   (+ x y)))))
(assert (not quoted-ok?) "a quoted (attune! ||) rejects arithmetic")
(assert (first (protect (eval '(defn always [x]
                                (attune! ||)
                                x))))
        "a quoted (attune! ||) accepts a silent body")

# ── Compile-time rejection: yield ────────────────────────────────────

(def [ok2? _]
  (protect (eval '(defn bad-yield []
                   (silent!)
                   (yield 1)))))
(assert (not ok2?) "silent! rejects yield at compile time")

# ── Compile-time rejection: I/O ──────────────────────────────────────

(def [ok3? _]
  (protect (eval '(defn bad-io [x]
                   (silent!)
                   (println x)))))
(assert (not ok3?) "silent! rejects I/O at compile time")

# ── Compile-time rejection: a parameter's signal passed on ───────────

(def [ok6? _]
  (protect (eval '(defn bad-call [f]
                   (silent!)
                   (f)))))
(assert (not ok6?) "silent! rejects a call to an unbounded parameter")

# ── Several ceilings: the body must fit each ─────────────────────────
#
# Counter-factual: the last ceiling replaced the first, so a body that
# raises compiled under (silent!) followed by (attune! :error).

(assert (compiles? "(defn one-ceiling [x y] (attune! :error) (+ x y))")
        "the looser ceiling alone admits the body")
(assert (not (compiles? "(defn two-ceilings [x y] (silent!) (attune! :error) (+ x y))"))
        "the earlier, tighter ceiling still applies")

# ── Compile-time rejection: a signal reached through a mutual cycle ──
#
# The raiser is not the function named at the call site — `foo` is silent on
# its own and only reaches `first` by way of `bar`. Inference seeds every
# binding in a letrec optimistically, so the cycle is only correct once the
# fixpoint has run: a loop that stops early reports `foo` as silent, this
# `(silent!)` compiles, and the program raises a broken proof at runtime
# instead.

(def [ok4? _]
  (protect (eval '(defn bad-mutual []
                   (silent!)
                   (letrec [foo (fn (n) (if (%eq n 0) 0 (bar (%sub n 1))))
                            bar (fn (n)
                                  (if (%eq n 0) (first (list)) (foo (%sub n 1))))]
                     (foo 3))))))
(assert (not ok4?) "silent! rejects a signal reached through a mutual cycle")

# The same cycle with no raiser in it must still compile — convergence must not
# inflate a silent cycle into a signalling one.

(def [ok5? _]
  (protect (eval '(defn good-mutual []
                   (silent!)
                   (letrec [foo (fn (n) (if (%eq n 0) 0 (bar (%sub n 1))))
                            bar (fn (n) (if (%eq n 0) 1 (foo (%sub n 1))))]
                     (foo 3))))))
(assert ok5? "silent! accepts a mutual cycle that raises nothing")

(println "all silent! compile-time enforcement tests passed")
