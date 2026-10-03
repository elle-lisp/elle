(elle/epoch 12)
# ── silence: compile-time enforcement ─────────────────────────────────
#
# (silence) enforces at compile time that the body's inferred signal
# fits within the declared ceiling.  Any excess bits are a compile error.
# Runtime enforcement (vm/call.rs) stays as defense-in-depth.

# ── Should compile: silence with pure control flow ───────────────────

(defn select [flag a b]
  (silence)
  (if flag a b))

(assert (= (select true 1 2) 1) "select true")
(assert (= (select false 1 2) 2) "select false")

(defn always [x]
  (silence)
  x)

(assert (= (always 42) 42) "always returns its argument")

# ── Compile-time rejection: arithmetic emits {:error} ────────────────

(def [ok? err]
  (protect (eval '(defn bad-add [x y]
                   (silence)
                   (+ x y)))))
(assert (not ok?) "silence rejects arithmetic at compile time")
(assert (string/contains? (get err :message) "may emit")
        "error mentions excess signal")

# ── Compile-time rejection: yield ────────────────────────────────────

(def [ok2? _]
  (protect (eval '(defn bad-yield []
                   (silence)
                   (yield 1)))))
(assert (not ok2?) "silence rejects yield at compile time")

# ── Compile-time rejection: I/O ──────────────────────────────────────

(def [ok3? _]
  (protect (eval '(defn bad-io [x]
                   (silence)
                   (println x)))))
(assert (not ok3?) "silence rejects I/O at compile time")

# ── Compile-time rejection: a signal reached through a mutual cycle ──
#
# The raiser is not the function named at the call site — `foo` is silent on
# its own and only reaches `first` by way of `bar`. Inference seeds every
# binding in a letrec optimistically, so the cycle is only correct once the
# fixpoint has run: a loop that stops early reports `foo` as silent, this
# `(silence)` compiles, and the program aborts at runtime instead. The
# self-recursive shape above already compiles to a rejection, so a pass here
# with a failure there would mean the cycle, not the enforcement, is broken.

(def [ok4? err4]
  (protect (eval '(defn bad-mutual []
                   (silence)
                   (letrec [foo (fn (n) (if (%eq n 0) 0 (bar (%sub n 1))))
                            bar (fn (n)
                                  (if (%eq n 0) (first (list)) (foo (%sub n 1))))]
                     (foo 3))))))
(assert (not ok4?) "silence rejects a signal reached through a mutual cycle")
(assert (string/contains? (get err4 :message) "may emit")
        "mutual-cycle rejection mentions excess signal")

# The same cycle with no raiser in it must still compile — convergence must not
# inflate a silent cycle into a signalling one.

(def [ok5? _]
  (protect (eval '(defn good-mutual []
                   (silence)
                   (letrec [foo (fn (n) (if (%eq n 0) 0 (bar (%sub n 1))))
                            bar (fn (n) (if (%eq n 0) 1 (foo (%sub n 1))))]
                     (foo 3))))))
(assert ok5? "silence accepts a mutual cycle that raises nothing")

(println "all silence compile-time enforcement tests passed")
