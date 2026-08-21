(elle/epoch 12)
# What a variadic call leaves behind.
#
# `(defn f [& xs] …)` collects its arguments into a list the caller never
# names, so nothing in the source says when that list dies. The answer
# has to be "when the call returns" — a variadic call in a loop is the
# most ordinary code there is, and every arithmetic operator in the
# prelude is one, so a list that outlived its call would make plain
# arithmetic accumulate.
#
# `arena/count` is the live object count across every region, so it
# answers the question directly and without naming a representation: run
# a loop of variadic calls and the count must come back to where it
# started, whatever the call allocates internally.
#
# The cases below vary what could keep the list alive: how many
# arguments it holds, whether the callee reads it, and whether the
# arguments are themselves heap values. The last case is the one that
# must NOT be reclaimed — a list the callee returns is reachable, and the
# count is expected to hold it.
#
# See docs/regions.md and src/vm/env.rs.

# Enough iterations that a per-call leak is unmistakable against the
# handful of objects a settled loop legitimately holds.
(def rounds 2000)

# How many live objects a loop may differ by once it is done. Nothing
# here retains, so the slack covers only what the measurement itself
# holds.
(def slack 16)

(defn drift [thunk]
  "Live objects gained across `thunk`."
  (let* [before (arena/count)
         _ (thunk)
         after (arena/count)]
    (%sub after before)))

(defn bounded [label thunk]
  "`thunk` gains no more than `slack` live objects."
  (let [d (drift thunk)]
    (assert (%lt d slack)
            (string label ": gained " (string d) " live objects over "
                    (string rounds) " calls"))
    (println "  " label ": " (string d))))

# ── The callees ──────────────────────────────────────────────────────

(defn ignores [& xs]
  "Never looks at its rest list."
  0)

(defn counts [& xs]
  "Reads the rest list without holding a walking cursor over it."
  (length xs))

(defn keeps [& xs]
  "Returns the rest list itself, so the caller decides its lifetime."
  xs)

# ── 1. Arity ─────────────────────────────────────────────────────────

(println "a variadic call gives back its rest list...")

(bounded "one argument, ignored"
         (fn []
           (let [@i 0]
             (while (%lt i rounds)
               (ignores i)
               (assign i (%add i 1))))))

(bounded "four arguments, ignored"
         (fn []
           (let [@i 0]
             (while (%lt i rounds)
               (ignores i i i i)
               (assign i (%add i 1))))))

(bounded "sixteen arguments, ignored"
         (fn []
           (let [@i 0]
             (while (%lt i rounds)
               (ignores i i i i i i i i i i i i i i i i)
               (assign i (%add i 1))))))

# ── 2. A callee that reads the list ──────────────────────────────────

(bounded "four arguments, measured by the callee"
         (fn []
           (let [@i 0]
             (while (%lt i rounds)
               (assert (= (counts i i i i) 4) "the callee saw four arguments")
               (assign i (%add i 1))))))

# ── 3. Heap values as arguments ──────────────────────────────────────

(bounded "four heap arguments, ignored"
         (fn []
           (let [@i 0]
             (while (%lt i rounds)
               (ignores (%pair i i) (%pair i i) (%pair i i) (%pair i i))
               (assign i (%add i 1))))))

# ── 5. The prelude's own variadic operators ──────────────────────────

(bounded "arithmetic, which is variadic all the way down"
         (fn []
           (let [@i 0
                 @sum 0]
             (while (%lt i rounds)
               (assign sum (+ sum i))
               (assign i (%add i 1)))
             (assert (> sum 0) "the sum accumulated"))))

# ── 6. A list the callee returns is reachable, and stays ─────────────

(println "a returned rest list is not reclaimed while it is held...")

(let* [held @[]
       gained (drift (fn []
                       (let [@i 0]
                         (while (%lt i 100)
                           (push held (keeps i i i i))
                           (assign i (%add i 1))))))]
  (assert (= (length held) 100) "every returned list was kept")
  (assert (> gained 100)
          (string "100 retained lists of four hold live objects, counted "
                  (string gained)))
  (each xs in held
    (assert (= (length xs) 4) "each retained list still holds four items"))
  (println "  100 retained lists of four: " (string gained)))

(println "rest args reclaim: every unheld rest list went back")
