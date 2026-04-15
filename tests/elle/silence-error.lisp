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

(def [ok? err] (protect (eval '(defn bad-add [x y] (silence) (+ x y)))))
(assert (not ok?) "silence rejects arithmetic at compile time")
(assert (string/contains? (get err :message) "may emit") "error mentions excess signal")

# ── Compile-time rejection: yield ────────────────────────────────────

(def [ok2? _] (protect (eval '(defn bad-yield [] (silence) (yield 1)))))
(assert (not ok2?) "silence rejects yield at compile time")

# ── Compile-time rejection: I/O ──────────────────────────────────────

(def [ok3? _] (protect (eval '(defn bad-io [x] (silence) (println x)))))
(assert (not ok3?) "silence rejects I/O at compile time")

(println "all silence compile-time enforcement tests passed")
