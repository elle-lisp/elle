# ── silence + runtime panic ──────────────────────────────────────────
#
# silence imposes no compile-time restriction — any code compiles
# inside a silenced function. The signal is clamped to silent.
# At runtime, if ANY signal fires, the process panics.

# ── Should compile: silence with arithmetic ──────────────────────────

(defn fast-add [x y]
  (silence)
  (+ x y))

(assert (= (fast-add 3 7) 10) "silenced add works")

(defn fast-square [x]
  (silence)
  (* x x))

(assert (= (fast-square 5) 25) "silenced square works")

# ── Should compile: silence with comparison + arithmetic ─────────────

(defn fast-abs [x]
  (silence)
  (if (> x 0) x (- 0 x)))

(assert (= (fast-abs -7) 7) "silenced abs works")
(assert (= (fast-abs 5) 5) "silenced abs positive")

# ── Should compile: silence with yield (runtime enforcement) ─────────

(def [ok? _] (protect (eval '(defn yields-but-silenced [] (silence) (yield 1)))))
(assert ok? "silence accepts yield at compile time")

# ── Runtime: error in silenced function causes abort ─────────────────
# Can't test abort from within Elle (process dies).
# Verify via subprocess that it aborts with diagnostic.

(def result (subprocess/system "target/debug/elle"
  ["--jit=0" "tests/elle/helpers/silence-abort.lisp"]))
(assert (not (= (get result :exit) 0)) "silenced function aborts on error")
(assert (string/contains? (get result :stderr) "silence violation")
  "abort message mentions silence violation")

# ── Runtime: yield in silenced function causes abort ─────────────────

(def result2 (subprocess/system "target/debug/elle"
  ["--jit=0" "tests/elle/helpers/silence-yield-abort.lisp"]))
(assert (not (= (get result2 :exit) 0)) "silenced function aborts on yield")
(assert (string/contains? (get result2 :stderr) "silence violation")
  "yield abort message mentions silence violation")

(println "all silence+runtime-panic tests passed")
