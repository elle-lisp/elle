(elle/epoch 12)
# audited: 2026-09-21
# A walk over a freshly-built collection assigns each element to a fn-local
# mutable and reads the binding afterward. The element arrives through a name of
# the walk's own — `each` binds one, `(let [p (get items idx)] …)` — and that
# name is the store's FEEDER: nothing reads it after the store, so the cell's
# store-site pin lands where the name dies and the 1-slot-container model holds
# (docs/impl/region/bindings.md § "A name the store consumes is not a second
# holder of the value").
#
# Reading the feeder as a second holder of the stored value refuses the model,
# and the unsuppressed baseline it falls back to cannot bound a loop: the
# element's producer reference is counted once per iteration while the binding
# chain drags its one release out to the cell's last use. Every element but the
# last is stranded, and each element lives in the collection's own region, so the
# whole collection strands — one region and two objects per entry, per call.
#
# Four faces. The first two say the walk computes what it should and that
# nothing frees the answer under its reader; the third measures the strand; the
# fourth is the control the strand is measured against — the same walk with the
# element read directly, whose value the compiler names itself.

(def @table @{:a 1 :b 2 :c 3})

# The shape: `each` over `(pairs table)`, whose elements live in the call's own
# fresh region. `u` is returned, so its content leaves with the caller.
(defn last-pair []
  (var u nil)
  (each p in (pairs table)
    (assign u p))
  u)

# The control: the element read stored straight into the cell, so the value's
# only name is the compiler's own ANF temp. Bounded before this shape was.
(defn last-pair-direct []
  (var u nil)
  (let [items (pairs table)]
    (var i 0)
    (var n (length items))
    (while (%lt i n)
      (assign u (get items i))
      (assign i (%add i 1))))
  u)

# ── 1. Correctness — the walk hands back an entry of the table ───────────
(assert (= (length (last-pair)) 2) "a pair is a key and a value")
(assert (has? table (get (last-pair) 0)) "the key is one of the table's")
(assert (= (get table (get (last-pair) 0)) (get (last-pair) 1))
        "the value is the one the table holds under that key")
(assert (= (length (last-pair-direct)) 2) "the control returns a pair too")

# ── 2. Not over-freed — the returned entry survives its callee ───────────
# The pin discharges the element's producer reference at the store, so the cell's
# own reference is what carries the value to the `Return`. Were the pin to
# consume the caller's reference instead, the reads below would see a freed
# entry. Churn between the call and the reads makes a recycled page likely, so a
# stale read shows as a wrong value rather than as intact bytes.
(def held (last-pair))
(def held-key (get held 0))
(def junk @[])
(each i in (range 0 64)
  (push junk (string "junk" i)))
(assert (= (length held) 2) "the returned entry survives the callee's release")
(assert (= (get held 0) held-key) "the returned key is intact after churn")
(assert (= (get held 1) (get table held-key))
        "the returned value is intact after churn")

# ── 3. Bounded — the rate is flat in the entry count ────────────────────
# Each element's producer reference dies at the store that took it, so a walk
# that reclaims measures the same growth over a 3-entry table and a 12-entry one.
# A walk that strands the collection grows with the entry count.
(defn drive [reps f]
  (var k 0)
  (while (%lt k reps)
    (f)
    (assign k (%add k 1))))

(defn growth [reps f]
  (drive 20 f)
  (var before (arena/region-count))
  (drive reps f)
  (%sub (arena/region-count) before))

(def @big @{})
(each i in (range 0 12)
  (put big (string "k" i) i))

(defn last-pair-big []
  (var u nil)
  (each p in (pairs big)
    (assign u p))
  u)

(let [small (growth 200 last-pair)
      large (growth 200 last-pair-big)]
  (assert (%lt small 100)
          (string "a walk's element name refuses the container model: live "
                  "count grew by " small " over 200 calls on 3 entries "
                  "(expected flat)"))
  (assert (%lt large (%add small 100))
          (string "the strand scales with the entry count: " small
                  " on 3 entries vs " large " on 12, over 200 calls")))

# ── 4. The control reads the same ───────────────────────────────────────
# The direct store was bounded before the feeder exclusion landed, so a
# regression that only moves this number is a different defect from the one
# above. The pair isolates the element NAME rather than the walk.
(assert (%lt (growth 200 last-pair-direct) 100)
        "the directly-stored element read is bounded")

(println "region-cell-feeder: ok")
