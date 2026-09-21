(elle/epoch 12)
# audited: 2026-09-21
# A fn-local 1-slot container whose STORED value carries a second name. The
# alias refuses nothing: the pin rule is a maximum and the alias's own reads
# extend the stored region's release, so the cell takes the counted store and
# each iteration's producer release stays inside the loop
# (docs/impl/region/bindings.md § "An aliased stored value takes the counted
# store"). Refused, the shape strands every named-then-stored element but the
# last — per call, permanently — which is the conditional-accumulate idiom the
# scheduler's unjoined-error scan takes (elle-lisp/elle#1186).
#
# Every subject builds its input INSIDE the call. The strand is the input's own
# elements, so an input hoisted out of the driver holds one region set for the
# whole run and the gauge reads flat over a real leak.

# ── 1. The minimal shape — name the element, store it in one arm ────────
(defn pick-first []
  (let [s [[7] [8] [9] [10]]]
    (let [@u nil]
      (def @i 0)
      (while (< i 4)
        (let [x (get s i)]
          (when (nil? u) (assign u x)))
        (assign i (%add i 1)))
      (get u 0))))

(assert (= (pick-first) 7)
        "the stored element reads back through the container after the loop")

# ── 2. The arm that never stores still releases every element ───────────
(defn pick-none []
  (let [s [[7] [8]]]
    (let [@u nil]
      (def @i 0)
      (while (< i 2)
        (let [x (get s i)]
          (when (not (nil? u)) (assign u x)))
        (assign i (%add i 1)))
      (nil? u))))

(assert (pick-none)
        "a condition that never fires leaves the container at its init")

# ── 3. The each faces — the macro's own arms store the element ──────────
# `each` walks a list with a cursor and an array by index, and its body's
# conditional assign is the shape the scheduler writes. The struct face takes
# the macro's default arm, whose walk sits inside a `while` inside a `match`
# arm — the content drop must leave both to run on every path.
(defn first-of-list []
  (let [xs (list [5] [6])]
    (let [@u nil]
      (each x in xs
        (when (nil? u) (assign u x)))
      (get u 0))))

(defn first-value-of []
  (let [t @{:a [1 2]}]
    (let [@u nil]
      (each [k v] in t
        (when (nil? u) (assign u v)))
      (get u 0))))

(assert (= (first-of-list) 5) "the each-list face hands the first element back")
(assert (= (first-value-of) 1) "the each-struct face hands a stored value back")

# ── 4. The alias outlives the store within its iteration ────────────────
# `x` is read AFTER the assign that stored it, so a pin that ran ahead of the
# read would free the element under it. The sum proves the reads saw live
# values.
(defn store-then-read []
  (let [s [[1] [2] [3]]]
    (let [@u nil]
      (def @acc 0)
      (def @i 0)
      (while (< i 3)
        (let [x (get s i)]
          (when (nil? u) (assign u x))
          (assign acc (+ acc (get x 0))))
        (assign i (%add i 1)))
      (list acc (get u 0)))))

(assert (= (store-then-read) (list 6 1))
        "every element reads back after the store that took it")

# ── 5. Bounded — the rate is flat whichever arm runs ────────────────────
(defn drive [f reps]
  (def @k 0)
  (while (< k reps)
    (f)
    (assign k (%add k 1))))

(defn growth [f]
  (drive f 20)
  (def before (arena/region-count))
  (drive f 200)
  (%sub (arena/region-count) before))

(let [stored (growth pick-first)
      skipped (growth pick-none)
      lists (growth first-of-list)
      structs (growth first-value-of)]
  (assert (%lt stored 100)
          (string "the storing arm strands its elements: live count grew by "
                  stored " over 200 calls (expected flat)"))
  (assert (%lt skipped 100)
          (string "the skipping arm strands its elements: live count grew by "
                  skipped " over 200 calls (expected flat)"))
  (assert (%lt lists 100)
          (string "the each-list face strands its input: live count grew by "
                  lists " over 200 calls (expected flat)"))
  (assert (%lt structs 100)
          (string "the each-struct face strands its snapshot: live count grew by "
                  structs " over 200 calls (expected flat)")))

(println "region-cell-aliased-store: ok")
