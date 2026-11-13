(elle/epoch 15)
# audited: 2026-11-11
# A compiled tail callee owns each argument exactly once: moved, borrowed, spliced, or collected by a rest parameter.
# docs/impl/region/mechanism.md
# docs/impl/region/relocate.md
#
# Each relay tail-calls a compiled callee, so the trampoline that owns the
# relay's activation runs the callee's compiled code. The callee's reference to
# each argument comes from a different place in each face:
#
# - a MOVED argument: the relay's own reference transfers, and the callee
#   mints none.
# - a BORROWED argument, an element of a destructured pair: the relay retains
#   it before the call, and that retain moves.
# - a SPLICED argument: the args array holds it, and the call reclaims the
#   array before the trampoline runs the callee. So the resolver mints the
#   callee's reference as it builds the pending call. An argument the callee's
#   `&` list or a cell takes over gets no such reference, so the resolver holds
#   it until the callee returns. Spliced out of a temporary array, nothing else
#   holds it once the array is reclaimed.
# - a REST argument: the callee's `&` list holds its own reference to each
#   member, so the moved one is surplus, and the trampoline releases it once
#   the callee returns. A value passed twice arrives with one reference and is
#   released by neither.
#
# A callee that mints a reference it was moved strands one string per call,
# which the gauge reads. A callee that mints none where the array held the only
# one, or a surplus released twice, frees a string the callee still reads, and
# region-jit-tail-dispatch-args-guardfree.lisp faults there.
#
# Each relay with an `-i` suffix takes a `&named` parameter, which the JIT
# refuses, so it runs in the interpreter and the interpreter's tail resolver
# builds the call.

(defn take [s]
  (string s "!"))

(defn take-rest [a & xs]
  (string a (first xs) (first (rest xs)) "!"))

(defn give-rest [a & xs]
  (let [r (yield a)]
    (string a (first xs) r)))

# The assignment puts `s` in a cell.
(defn take-cell [@s]
  (assign s (string s "!"))
  s)

(defn relay-m [n]
  (take (string "m" n)))

(defn relay-mi [n &named unused]
  (take (string "m" n)))

(defn relay-b [p]
  (let [[a _] p]
    (take a)))

(defn relay-s [xs]
  (take ;xs))

(defn relay-si [xs &named unused]
  (take ;xs))

(defn relay-st [n]
  (take-rest ;[(string "a" n) (string "b" n) (string "c" n)]))

(defn relay-sti [n &named unused]
  (take-rest ;[(string "a" n) (string "b" n) (string "c" n)]))

(defn relay-sc [n]
  (take-cell ;[(string "c" n)]))

(defn relay-r [n]
  (take-rest (string "a" n) (string "b" n) (string "c" n)))

(defn relay-ri [n &named unused]
  (take-rest (string "a" n) (string "b" n) (string "c" n)))

(defn relay-alias [n]
  (let [s (string "a" n)]
    (take-rest s s s)))

(defn relay-y [n]
  (give-rest (string "a" n) (string "b" n)))

# ── the faces ───────────────────────────────────────────────────────────────
# `nest` runs in the interpreter and calls a relay in call position. A compiled
# relay then hands its tail call back to the interpreter's call into compiled
# code, and an interpreted one runs on the interpreter's own dispatch loop.
(defn nest [f x &named unused]
  (let [r (f x)]
    r))

(defn same [got want label]
  (assert (= got want) (string label ": " got)))

(defn moved [n]
  (let [want (string "m" n "!")]
    (same (relay-m n) want "a moved argument")
    (same (nest relay-m n) want "a moved argument, nested")
    (same (relay-mi n) want "a moved argument, interpreted")
    (same (nest relay-mi n) want "a moved argument, interpreted and nested")))

(defn borrowed [n]
  (let [p [(string "b" n) n]]
    (same (relay-b p) (string "b" n "!") "a borrowed argument")
    (same (nest relay-b p) (string "b" n "!") "a borrowed argument, nested")
    (same (get p 0) (string "b" n) "the pair keeps its element")))

(defn spliced [n]
  (let [xs [(string "s" n)]
        want (string "s" n "!")]
    (same (relay-s xs) want "a spliced argument")
    (same (nest relay-s xs) want "a spliced argument, nested")
    (same (relay-si xs) want "a spliced argument, interpreted")
    (same (nest relay-si xs) want "a spliced argument, interpreted and nested")
    (same (get xs 0) (string "s" n) "the array keeps its element")))

(defn held [n]
  (let [want (string "a" n "b" n "c" n "!")]
    (same (relay-st n) want "a spliced rest argument")
    (same (nest relay-st n) want "a spliced rest argument, nested")
    (same (relay-sti n) want "a spliced rest argument, interpreted")
    (same (relay-sc n) (string "c" n "!") "a spliced argument a cell takes")))

(defn collected [n]
  (let [want (string "a" n "b" n "c" n "!")]
    (same (relay-r n) want "a rest argument")
    (same (nest relay-r n) want "a rest argument, nested")
    (same (relay-ri n) want "a rest argument, interpreted")
    (same (nest relay-ri n) want "a rest argument, interpreted and nested")
    (same (relay-alias n) (string "a" n "a" n "a" n "!")
          "one value in every position")))

(defn suspended [n]
  (let [f (fiber/new (fn [] [(relay-y n)]) |:yield|)]
    (same (fiber/resume f) (string "a" n) "the rest callee yields")
    (same (fiber/resume f "!") [(string "a" n "b" n "!")]
          "the rest callee reads its list after the resume")))

# ── drive ───────────────────────────────────────────────────────────────────
(def window 200)

(defn measure [face]
  (def @i 0)
  (while (%lt i 30)
    (face i)
    (assign i (%add i 1)))
  (jit/rejections)
  (def before (arena/count))
  (def @j 0)
  (while (%lt j window)
    (face j)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# A surplus reference strands at least one string per call, so a leak reads at
# least `window`.
(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(bounded? (measure moved) "a moved argument")
(bounded? (measure borrowed) "a borrowed argument")
(bounded? (measure spliced) "a spliced argument")
(bounded? (measure held) "a spliced argument a list or a cell takes")
(bounded? (measure collected) "a rest argument")
(bounded? (measure suspended) "a rest argument across a yield")

(when (vm/config :jit)
  (assert (and (jit? take) (jit? take-rest) (jit? give-rest) (jit? take-cell))
          "precondition: the callees compile")
  (assert (and (jit? relay-m) (jit? relay-b) (jit? relay-s) (jit? relay-st)
               (jit? relay-sc) (jit? relay-r) (jit? relay-alias) (jit? relay-y))
          "precondition: the compiled relays compile")
  (assert (not (or (jit? relay-mi) (jit? relay-si) (jit? relay-sti)
                   (jit? relay-ri) (jit? nest)))
          "precondition: nest and the -i relays stay interpreted"))

(println "region-jit-tail-dispatch-args: ok")
