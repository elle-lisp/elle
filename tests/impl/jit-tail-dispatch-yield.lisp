(elle/epoch 15)
# audited: 2026-11-11
# A compiled tail callee that yields parks its own frame with what the trampoline's activation owes, and the resume finishes it.
# docs/impl/region/owner.md
#
# `give` yields its argument and answers it joined to the resume value. Each
# relay reaches `give` by a tail call, so the trampoline that owns the relay's
# activation runs `give`, compiled, in place of the relay's body. The compiled
# `give` parks a frame of its own and leaves the trampoline's activation behind
# it, never to run again. What that activation owes has to move into the frame
# `give` parks: the resume's completion of `give` is then the clean break that
# releases it, and a discard of the suspended fiber releases it too.
#
# `relay-d` owes something: it tail-calls a closure it built, whose release
# the frame replacement strands, so the activation takes the release over
# (docs/impl/region/owner.md). The JIT refuses a function that builds a
# closure, so `relay-d` runs in the interpreter. `relay` is compiled and owes
# nothing, and `relay-c` calls `give` in call position as the control.
# `relay-n` reaches `relay-d` through `nest`, which also runs in the
# interpreter.
#
# The sidecar compiles on the VM thread at each function's first call, and
# turns the graft off, so each tail call stays a call.
#
# The values are right however the dues are handled. A release that never runs
# strands one closure per call, which only the gauge sees. A release that runs
# twice, or while the frame still reads what it frees, faults under
# jit-tail-dispatch-yield-guardfree.lisp, and each face reads its argument
# after the resume.

(defn give [s]
  (let [r (yield s)]
    (string s r)))

(defn relay [s]
  (give s))

(defn relay-d [s]
  ((fn [] (give s))))

(defn relay-c [s]
  (let [r (give s)]
    r))

# `nest` runs in the interpreter, and calls `relay-d` in call position, so the
# interpreter runs `relay-d` on its own dispatch loop and that loop's
# trampoline runs the tail callee.
(defn nest [f s &named unused]
  (let [r (f s)]
    r))

(defn relay-n [s]
  (nest relay-d s))

# ── the faces ───────────────────────────────────────────────────────────────
(defn resumed [via n]
  (let [s (string "v" n)
        f (fiber/new (fn [] [(via s)]) |:yield|)]
    (assert (= (fiber/resume f) s) "the callee yields its argument")
    (assert (= (fiber/resume f "!") [(string "v" n "!")])
            "the resume answers the yield inside the compiled callee")
    (assert (= s (string "v" n)) "the argument outlives the resume")))

(defn dropped [via n]
  (let [s (string "v" n)
        f (fiber/new (fn [] [(via s)]) |:yield|)]
    (assert (= (fiber/resume f) s) "the callee yields its argument")
    (assert (= s (string "v" n)) "the argument outlives the discard")))

# ── drive ───────────────────────────────────────────────────────────────────
(def window 200)

(defn measure [face via]
  (def @i 0)
  (while (%lt i 30)
    (face via i)
    (assign i (%add i 1)))
  (jit/rejections)
  (def before (arena/count))
  (def @j 0)
  (while (%lt j window)
    (face via j)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# A lost release strands at least one closure per suspended call, so a leak
# reads at least `window`.
(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(bounded? (measure resumed relay-c) "control: a yield in call position")
(bounded? (measure resumed relay) "a compiled relay, resumed")
(bounded? (measure resumed relay-d) "an interpreted relay that owes, resumed")
(bounded? (measure resumed relay-n) "a nested relay that owes, resumed")
(bounded? (measure dropped relay) "a compiled relay, discarded")
(bounded? (measure dropped relay-d) "an interpreted relay that owes, discarded")

(when (vm/config :jit)
  (assert (and (jit? give) (jit? relay)) "precondition: give and relay compile")
  (assert (not (or (jit? relay-d) (jit? nest)))
          "precondition: relay-d and nest stay interpreted"))

(println "jit-tail-dispatch-yield: ok")
