(elle/epoch 12)
# A spliced call's arguments come out of an array the convention owns
# (docs/impl/region/mechanism.md § "A spliced call's arguments come out of an
# array the convention owns").
#
# A call with a spliced argument — `(f ;args)`, and the `apply` it underlies —
# cannot push its arguments onto the operand stack, because how many there are
# is a runtime fact. The lowerer builds them into a fresh `@array` and hands
# that array to `CallArrayMut`/`TailCallArrayMut`. No binding of the program
# ever names that array, so nothing in the emitted code releases it: the array
# is the calling convention's, and the call that consumes it releases it.
#
# THE TRAP the counts below guard. The array and the call are two allocations,
# and a static region slot names ONE allocation execution between drops
# (docs/impl/region/model.md § "The per-execution region model"). Giving the
# array the CALL's slot leaves the array's physical region orphaned the moment
# the call maps its own mint over the same slot — unreachable by the value route
# (no binding names it) and by the abandoned-frame walk (the slot now maps
# something else), so it survives to process teardown.
#
# THE COUNTER-FACTUAL a two-column reading catches. A spliced TAIL call to a
# closure replaces the frame, so every release the lowerer emits after
# `TailCallArrayMut` is dead. Reading the array's own reclaim as the whole
# defect leaves those behind — one region per source the splice read, on top of
# the array. That is why (d) and (g) below are separate subjects from (c): a
# native tail callee keeps the frame and runs that block, so it reads bounded
# even while a closure callee strands it.
#
# This file is the LEAK gauge — an `arena/region-count` delta over a fixed
# window, BOUNDED for every subject. The soundness complement is
# region-splice-args-uaf.lisp.

(def window 400)

(def shared (string "subject"))

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-count) before))

(defn take-one [x]
  (length x))
(defn take-two [x y]
  (%add (length x) (length y)))
(defn take-rest [& rest]
  (length rest))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) NON-tail splice, native callee. The frame is never replaced, so the only
# region with no release is the args array itself.
(defn a-nontail-native []
  (let [args [shared]]
    (let [n (length ;args)]
      (%add n 0))))

# (b) NON-tail splice, closure callee. The callee mints its own reference to
# each parameter, so the array's counted edge is surplus and its reclaim must
# cascade exactly once.
(defn b-nontail-closure []
  (let [args [shared]]
    (let [n (take-one ;args)]
      (%add n 0))))

# (c) TAIL splice, native callee. A native pushes no bytecode frame, so the
# post-`TailCallArrayMut` block runs and carries the frame's own releases.
(defn c-tail-native []
  (let [args [shared]]
    (length ;args)))

# (d) TAIL splice, CLOSURE callee — the frame-replacing shape. The array is one
# stranded region; the source the splice read is another, and only the
# relocation ahead of the frame replacement reclaims it.
(defn d-tail-closure []
  (let [args [shared]]
    (take-one ;args)))

# (e) `apply`, which lowers to the same splice path.
(defn e-apply []
  (let [args [shared]]
    (apply take-one args)))

# (f) a splice whose source is built at the call — nothing outside the call ever
# names the array OR its source.
(defn f-tail-literal []
  (take-one ;[shared]))

# (g) a MIXED call: one plain argument beside a spliced one. The plain argument
# is pushed into the same array, so its own reference and the array's must not
# be counted for each other.
(defn g-tail-mixed []
  (let [args [shared]]
    (take-two shared ;args)))

# (h) a VARIADIC callee: the spliced arguments land in the rest parameter's
# collected list, which takes an incref of its own. The array's reclaim and the
# list's release name the same regions and must each run once.
(defn h-tail-variadic []
  (let [args [shared shared]]
    (take-rest ;args)))

# controls ─────────────────────────────────────────────────────────────────────

# (i) the identical computation with no splice in it: the plain call path, whose
# rate this file's subjects must reach.
(defn i-plain []
  (let [args [shared]]
    (take-one (get args 0))))

# (j) the array literal alone, with no call spliced from it — proves the source
# collection is not what any subject's rate measures.
(defn j-source-only []
  (let [args [shared]]
    (length args)))

# measurement ──────────────────────────────────────────────────────────────────

(def d-a (measure a-nontail-native 20 window))
(def d-b (measure b-nontail-closure 20 window))
(def d-c (measure c-tail-native 20 window))
(def d-d (measure d-tail-closure 20 window))
(def d-e (measure e-apply 20 window))
(def d-f (measure f-tail-literal 20 window))
(def d-g (measure g-tail-mixed 20 window))
(def d-h (measure h-tail-variadic 20 window))
(def d-i (measure i-plain 20 window))
(def d-j (measure j-source-only 20 window))

(println "region-splice-args over " window " iters (region deltas):")
(println "  a nontail-native  " d-a)
(println "  b nontail-closure " d-b)
(println "  c tail-native     " d-c)
(println "  d tail-closure    " d-d)
(println "  e apply           " d-e)
(println "  f tail-literal    " d-f)
(println "  g tail-mixed      " d-g)
(println "  h tail-variadic   " d-h)
(println "  i plain           " d-i " (control)")
(println "  j source-only     " d-j " (control)")

(assert (%lt d-i 40)
        (concat "control: the plain call path must be bounded, delta="
                (number->string d-i)))
(assert (%lt d-j 40)
        (concat "control: the splice source alone must be bounded, delta="
                (number->string d-j)))

(assert (%lt d-a 40)
        (concat "a non-tail spliced call strands its args array, delta="
                (number->string d-a)))
(assert (%lt d-b 40)
        (concat "a non-tail spliced call to a closure strands its args array, "
                "delta=" (number->string d-b)))
(assert (%lt d-c 40)
        (concat "a spliced tail call to a native strands its args array, delta="
                (number->string d-c)))
(assert (%lt d-d 40)
        (concat "a spliced tail call to a closure strands its args array and "
                "the source the splice read, delta=" (number->string d-d)))
(assert (%lt d-e 40)
        (concat "`apply` strands its args array, delta=" (number->string d-e)))
(assert (%lt d-f 40)
        (concat "a spliced tail call over a literal source strands it, delta="
                (number->string d-f)))
(assert (%lt d-g 40)
        (concat "a mixed plain/spliced tail call strands its args array, delta="
                (number->string d-g)))
(assert (%lt d-h 40)
        (concat "a spliced tail call to a variadic callee strands its args "
                "array, delta=" (number->string d-h)))

(println "region-splice-args: ok")
