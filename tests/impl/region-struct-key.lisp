(elle/epoch 12)
# A struct key's region is counted only when it is another region.
#
# A key that is actually stored is interned into the container's own region
# (docs/impl/values.md § "Struct keys"), which makes it a self-edge: the RC and
# the outgoing-edge table count it on neither side, matching the free-time
# cascade's `own_id` filter. This file drives both halves of that ledger from
# Elle, because counting a self-edge on one side only fails two different ways
# and each way needs its own witness.
#
# LEAK half — `put` of a string or array key increfed the container's own
# region. The free cascade never releases a self-edge, so the whole region
# leaked, one per call, and only a matching `del` could cancel it. Measured in
# regions (`arena/region-count`), deterministically.
#
# UAF half — `del` of a key a CONSTRUCTOR interned decrefed the container's
# region. The literal `@{"k" 1}` builds through `build::struct_mut_from`, whose
# alloc-time scan counts nothing for a co-region key, so that decref took the
# struct's sole reference to zero and freed the struct under its own binding.
#
# The trap: a `del` that frees its own container does NOT reliably crash. The
# freed page keeps its old bytes and reads through it still answer correctly
# until something reuses the page — under `--trace=scrub` the page is blanked
# and the read panics at the deref, which is how CI caught this, but a plain
# run of the same code can pass. So the witness here is the region gauge, which
# is exact on every platform and every build: a `del` frees no region, because
# the region it would free is the one the struct is still living in. The
# readback assertions come after, for the semantics.

(defn region-delta [f iters]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i iters)
    (f i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# ── LEAK: a put of a heap key holds no region ─────────────────────
# The keyword drive is the control: a keyword key carries no `Value` at all, so
# neither ledger has anything to count for it. A rate that moves for the string
# and array drives but not for this one names the key's heap value as the leak.

(defn put-keyword [k]
  (let [m @{}]
    (put m :k 1)
    0))
(defn put-string [k]
  (let [m @{}]
    (put m "kk" 1)
    0))
(defn put-array [k]
  (let [m @{}]
    (put m [1 2] 1)
    0))
(defn put-then-del-array [k]
  (let [m @{}]
    (put m [1 2] 1)
    (del m [1 2])
    0))

(let [d-keyword (region-delta put-keyword 200)
      d-string (region-delta put-string 200)
      d-array (region-delta put-array 200)
      d-cancelled (region-delta put-then-del-array 200)]
  (assert (%lt d-keyword 20)
          (concat "keyword-key put leak: delta=" (number->string d-keyword)))
  (assert (%lt d-string 20)
          (concat "string-key put leak: delta=" (number->string d-string)))
  (assert (%lt d-array 20)
          (concat "array-key put leak: delta=" (number->string d-array)))
  (assert (%lt d-cancelled 20)
          (concat "put+del array-key leak: delta=" (number->string d-cancelled))))

# ── UAF: del of a constructor-interned key frees no region ─────────
# Each `del` below removes a key the LITERAL interned, so the remove half has
# no incref of its own to release. The keyword drive is the control again: its
# key carries no `Value`, so its `del` releases nothing under either rule.
#
# The trap: every VALUE here is an immediate. A `del` that removes a heap value
# releases that value's region, and freeing it is correct — so a heap value
# moves the gauge by one on its own and hides whatever the key did.

(let [m @{:b 1 :a 2}
      before (arena/region-count)]
  (del m :b)
  (assert (= (arena/region-count) before) "del of a keyword key frees no region")
  (assert (not (has? m :b)) "del removes the keyword key")
  (assert (= (get m :a) 2) "and leaves the other entry readable"))

(let [m @{"kk" 1 :a 2}
      before (arena/region-count)]
  (del m "kk")
  (assert (= (arena/region-count) before)
          "del of an interned string key frees no region")
  (assert (not (has? m "kk")) "del removes the interned string key")
  (assert (= (get m :a) 2) "the struct survives the del of a string key")
  (assert (= (length m) 1) "and reports the one entry it has left"))

(let [m @{[1 2] 1 :a 2}
      before (arena/region-count)]
  (del m [1 2])
  (assert (= (arena/region-count) before)
          "del of an interned array key frees no region")
  (assert (not (has? m [1 2])) "del removes the interned array key")
  (assert (= (get m :a) 2) "the struct survives the del of an array key")
  (assert (= (keys m) (quote (:a))) "and lists the one key it has left"))

# The last entry: the del empties the struct, so nothing but the container
# itself is left to read through.
(let [m @{"only" 1}
      before (arena/region-count)]
  (del m "only")
  (assert (= (arena/region-count) before)
          "del of the last interned key frees no region")
  (assert (= (length m) 0) "the emptied struct is still readable")
  (put m "again" 2)
  (assert (= (get m "again") 2) "and still takes a new interned key"))

# ── the same shape past the JIT threshold ─────────────────────────
# The store funnels are shared by every tier, but the region a literal is built
# in is chosen by the emitter, so the compiled tier is a separate witness.

(defn del-then-read []
  (let [m @{[3 4] 9 :a 1}]
    (del m [3 4])
    (get m :a)))

(var last 0)
(var i 0)
(while (%lt i 500)
  (assign last (del-then-read))
  (assign i (%add i 1)))
(assert (= last 1) "the del-then-read shape holds on the compiled tier too")

(println "region-struct-key: all tests passed")
