(elle/epoch 12)
# Soundness complement of region-splice-args.lisp
# (docs/impl/region/mechanism.md § "A spliced call's arguments come out of an
# array the convention owns"). Run under `--trace=guardfree` by the subprocess
# pin `region_splice_args_uaf` in tests/integration/elle_scripts.rs.
#
# The args array a spliced call builds holds one counted reference per element,
# and the call reclaims the array once the callee holds what it needs. That
# reclaim is a release on a path that ran none before, so it owes what any new
# release owes: the reference it drops must be the array's own and no one
# else's.
#
# Four faces have to survive it.
#
# 1. The ARGUMENT the callee still reads. The callee mints its own reference to
#    every parameter, so the array's cascade must leave that one standing —
#    including for a spliced TAIL call, where the frame that built the array is
#    replaced before the callee runs a single instruction.
# 2. The SOURCE the splice read. A spliced tail call moves nothing, so the
#    frame's release of the source is relocated ahead of the frame replacement
#    and now runs BEFORE the callee reads the elements. The array's own
#    reference is what stands between the two.
# 3. The RESULT handed back through the array's reclaim. A pass-through native
#    returns a value living inside an argument, and the reclaim runs after the
#    call — so the pass-through retain, not the array, must be what keeps it.
# 4. An OUTER holder. The value the splice read is still named by the binding it
#    came from, whose reference the reclaim must leave alone.
#
# Every read below happens AFTER the spliced call returned, so an over-release
# faults at the deref (guardfree) or trips the generation check.

(defn take-one [x]
  (length x))
(defn take-two [x y]
  (%add (length x) (length y)))
(defn take-rest [& rest]
  (length rest))
(defn hand-back [x]
  x)

# ── 1. the argument the callee reads, in tail position ────────────────────────
# `(take-one ;args)` in tail position replaces this frame. The callee reads its
# parameter after the array is gone, so the reference it minted must be live.

(defn tail-closure [tag]
  (let [args [(string "elem-" tag)]]
    (take-one ;args)))

(var i 0)
(while (< i 40)
  (assert (< 0 (tail-closure i))
          "a spliced tail call's argument must survive the array's reclaim")
  (assign i (+ i 1)))

# ── 2. the source the splice read, released ahead of the frame replacement ────
# Two elements from one source, so the source's own release — relocated ahead of
# the tail call — drops a reference the array's edges must not be counted for.

(defn tail-two-from-source [tag]
  (let [args [(string "a-" tag) (string "b-" tag)]]
    (take-two ;args)))

(assign i 0)
(while (< i 40)
  (assert (< 0 (tail-two-from-source i))
          "a spliced tail call's source may be released before the callee reads")
  (assign i (+ i 1)))

# ── 3. the result handed back out of an argument ──────────────────────────────
# `hand-back` returns its parameter, so the value the caller reads lives in a
# region the array also referenced. The caller's read happens after the reclaim.

(defn tail-passthrough [tag]
  (let [args [(string "through-" tag)]]
    (hand-back ;args)))

(assign i 0)
(while (< i 40)
  (assert (= (length (tail-passthrough i)) (length (string "through-" i)))
          "a spliced call's pass-through result must survive the array's reclaim")
  (assign i (+ i 1)))

# ── 4. the outer holder, in call position ─────────────────────────────────────
# A NON-tail splice: the frame keeps its own reference to the source and reads
# it after the call, so the array's reclaim must drop only the array's.

(defn nontail-then-read [tag]
  (let [src (string "outer-" tag)]
    (let [args [src]]
      (let [n (take-one ;args)]
        [n (length src) (length (get args 0))]))))

(assign i 0)
(while (< i 40)
  (let [r (nontail-then-read i)]
    (assert (= (get r 0) (get r 1))
            "the splice source must still be readable after the call")
    (assert (= (get r 1) (get r 2))
            "the splice source's own array must still hold its element"))
  (assign i (+ i 1)))

# ── 5. the variadic callee: the rest list holds its own reference ─────────────
# The collected rest list increfs each element, so the array's cascade and the
# list's release name the same regions. Reading the list's elements inside the
# callee proves neither ran twice.

(defn rest-sum [& rest]
  (var total 0)
  (each x in rest
    (assign total (%add total (length x))))
  total)

(defn tail-variadic [tag]
  (let [args [(string "r1-" tag) (string "r2-" tag)]]
    (rest-sum ;args)))

(assign i 0)
(while (< i 40)
  (assert (< 0 (tail-variadic i))
          "a spliced variadic call's rest elements must survive the reclaim")
  (assign i (+ i 1)))

# ── 6. `apply`, and a mutable source the callee keeps reading ─────────────────
# The source is an `@array` the caller still owns; `apply` splices it into a
# fresh args array, and both must be intact afterwards.

(defn apply-then-read [tag]
  (let [src @[(string "m-" tag)]]
    (let [n (apply take-one src)]
      [n (length (get src 0))])))

(assign i 0)
(while (< i 40)
  (let [r (apply-then-read i)]
    (assert (= (get r 0) (get r 1))
            "an `apply` source must survive its args array's reclaim"))
  (assign i (+ i 1)))

# ── 7. a spliced call that RAISES before the array is consumed ────────────────
# `ArrayMutExtend` over a non-sequence raises where the array exists and the
# call has not run, so the abandoned frame reclaims it. The catcher's own values
# must be untouched by that walk.

(defn bad-splice [tag]
  (let [keep (string "keep-" tag)]
    (let [r (try
              (take-one ;tag)
              (catch e :caught))]
      [r (length keep)])))

(assign i 0)
(while (< i 40)
  (let [r (bad-splice i)]
    (assert (= (get r 0) :caught) "a bad splice source must raise")
    (assert (< 0 (get r 1)) "the catching frame's own value must survive"))
  (assign i (+ i 1)))

# ── 8. the callee SUSPENDS inside the spliced call ────────────────────────────
# THE TRAP. A park SNAPSHOTS the activation's region map, and the resume checks
# every slot in that snapshot against the region's generation. So the args
# array's slot has to be claimed BEFORE the callee can park — a claim after it
# clears only the live map, leaving the parked copy naming a region the reclaim
# then frees, and the resume detonates the uncounted-borrow check.
#
# Both call positions park here: the `let`-bound call parks the splicing frame
# itself, and the tail call parks the callee's, whose args came out of an array
# the replaced frame built.

(defn yield-then-count [x]
  (yield x)
  (length x))

(defn nontail-splice-park [tag]
  (let [args [(string "park-" tag)]]
    (let [f (fiber/new (fn []
                         (let [v (yield-then-count ;args)]
                           (%add v 0))) |:yield|)]
      (let [handed (fiber/resume f)]
        [(length handed) (fiber/resume f)]))))

(defn tail-splice-park [tag]
  (let [args [(string "tpark-" tag)]]
    (let [f (fiber/new (fn [] (yield-then-count ;args)) |:yield|)]
      (let [handed (fiber/resume f)]
        [(length handed) (fiber/resume f)]))))

(assign i 0)
(while (< i 40)
  (let [r (nontail-splice-park i)]
    (assert (= (get r 0) (get r 1))
            "a spliced call's parked frame must resume onto a live argument"))
  (let [r (tail-splice-park i)]
    (assert (= (get r 0) (get r 1))
            "a spliced tail call's parked callee must resume onto a live argument"))
  (assign i (+ i 1)))

(println "region-splice-args-uaf: ok")
