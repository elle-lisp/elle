(elle/epoch 12)
# Two sequential loops over ONE reassigned mutable. Functionalization splits the
# source name into one version per loop and initializes each from the previous
# one, so the forwarding edge is a CHAIN: `last#2 <- last#1 <- last#0`. The
# middle version carries a 1-slot cell of its own, and the reference the chain
# forwards has exactly one holder at a time — the cell that received it last
# (docs/impl/region/bindings.md § "A chain of forwarding edges hands one
# reference along, so the fold follows it whole").
#
# Three faces, and the shape needs all three because the two failure modes point
# opposite ways. Releasing the forwarded reference twice frees a live value;
# releasing it nowhere strands one region per iteration of BOTH loops, since the
# chain's links stand or fall together.

(defn chain [n]
  (var last (array 0 0))
  (var i 0)
  (while (%lt i n)
    (assign last (array i 7))
    (assign i (%add i 1)))
  (var j 0)
  (while (%lt j n)
    (assign last (array j 9))
    (assign j (%add j 1)))
  (get last 1))

# ── 1. Correctness — the chain's content survives every release ─────────
# `(chain 0)` runs neither loop, so the init value is forwarded twice and read
# back through both cells; `(chain 5)` reaches the second loop's own content.
(assert (= (chain 0) 0) "an unrun pair of loops reads the init value back")
(assert (= (chain 5) 9) "the second loop's final value reads back")

# ── 2. Not over-freed — every value the loops produced stays readable ────
# Each stored value also goes into a keeper array (a runtime-counted funnel
# store), so a value released twice would be freed under a live holder.
(defn chain-keep [n]
  (var out @[])
  (var last (array 0 0))
  (var i 0)
  (while (%lt i n)
    (assign last (array i 7))
    (%array-push out last)
    (assign i (%add i 1)))
  (var j 0)
  (while (%lt j n)
    (assign last (array j 9))
    (%array-push out last)
    (assign j (%add j 1)))
  (%freeze out))

(def kept (chain-keep 4))
(assert (= (length kept) 8) "the keeper holds one value per iteration")
(assert (= (get (get kept 0) 1) 7) "the first loop's first value is intact")
(assert (= (get (get kept 3) 0) 3) "the first loop's last value is intact")
(assert (= (get (get kept 7) 1) 9) "the second loop's last value is intact")

# ── 3. Bounded — the rate is flat in the iteration count ────────────────
# Each iteration's value dies at the next overwrite, so a chain that reclaims
# measures the same growth at n=8 and n=16. A chain that strands the forwarded
# reference grows with n, in BOTH loops.
#
# `read-cell` is the second bounded shape: an UNCOUNTED opcode read of the cell
# (`%get`) borrows out of whatever the cell holds now, and the cell's own
# reference — released at its content drop, which lands at or after every read —
# is what protects the borrow. Extending each stored value's release to the read
# instead would put one release behind a loop that stores N.
(defn read-cell [n]
  (var last (array 0 0))
  (var i 0)
  (while (%lt i n)
    (assign last (array i 7))
    (assign i (%add i 1)))
  (var j 0)
  (while (%lt j n)
    (assign last (array j 9))
    (assign j (%add j 1)))
  (%get last 1)
  0)

(defn drive [which reps n]
  (var k 0)
  (while (%lt k reps)
    (if which (chain n) (read-cell n))
    (assign k (%add k 1))))

(defn growth [which reps n]
  (drive which 20 n)
  (var before (arena/region-count))
  (drive which reps n)
  (%sub (arena/region-count) before))

(each which in (list true false)
  (let [small (growth which 200 8)
        large (growth which 200 16)]
    (assert (%lt small 400)
            (string "forwarding chain strands regions: live count grew by "
                    small " over 200 calls at n=8 (expected flat)"))
    (assert (%lt large (%add small 100))
            (string "forwarding chain's rate scales with the iteration count: "
                    small " at n=8 vs " large " at n=16 over 200 calls"))))

(println "region-cell-forward-chain: ok")
