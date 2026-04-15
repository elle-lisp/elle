# ── muffle: compile-time signal absorption ────────────────────────────
#
# (muffle :signal) absorbs specific signals from the body, allowing
# (silence) functions to contain operations that declare those signals.
# Runtime enforcement (vm/call.rs abort) catches the case where the
# muffled signal actually fires.

# ── muffle :error allows arithmetic in silent functions ──────────────

(defn fast-add [x y]
  (silence)
  (muffle :error)
  (+ x y))

(assert (= (fast-add 3 7) 10) "muffled add works")

(defn fast-abs [x]
  (silence)
  (muffle :error)
  (if (> x 0) x (- 0 x)))

(assert (= (fast-abs -7) 7) "muffled abs works")
(assert (= (fast-abs 5) 5) "muffled abs positive")

# ── muffle with set literal ──────────────────────────────────────────

(defn fast-square [x]
  (silence)
  (muffle |:error|)
  (* x x))

(assert (= (fast-square 5) 25) "muffled set literal works")

# ── muffle without silence: absorbs from inferred signal ─────────────

(defn add-quiet [x y]
  (muffle :error)
  (+ x y))

(assert (= (add-quiet 3 7) 10) "muffle without silence works")

# ── silence alone still rejects unmuffled signals ────────────────────

(def [ok? _] (protect (eval '(defn bad [x y] (silence) (+ x y)))))
(assert (not ok?) "silence without muffle still rejects arithmetic")

# ── muffle doesn't help with unmuffled signals ───────────────────────

(def [ok2? _] (protect (eval '(defn bad2 [] (silence) (muffle :error) (yield 1)))))
(assert (not ok2?) "muffle :error doesn't help with :yield")

# ── muffle outside function is an error ──────────────────────────────

(def [ok3? _] (protect (eval '(muffle :error))))
(assert (not ok3?) "muffle outside function is an error")

(println "all muffle tests passed")
