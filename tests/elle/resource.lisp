(elle/epoch 9)
# Resource consumption measurement tests
#
# Uses lib/resource.lisp to measure deterministic resource counters
# across representative scenarios. Output is machine-parseable for CI
# regression detection.

(def res ((import-file "lib/resource.lisp")))
(def checked? (vm/config :checked-intrinsics))

# ── Helper definitions ────────────────────────────────────────────

(defn fib [n]
  (if (%lt n 2)
    n
    (%add (fib (%sub n 1)) (fib (%sub n 2)))))

(defn build-list [n acc]
  (if (= n 0) acc (build-list (%sub n 1) (pair n acc))))

(defn sum-list [lst acc]
  (if (empty? lst)
    acc
    (sum-list (rest lst) (%add acc (first lst)))))

# ── Scenarios ─────────────────────────────────────────────────────

(def scenarios
  [["fib-15" (fn [] (fib 15))]

   ["pair-build-100" (fn [] (build-list 100 (list)))]

   ["pair-sum-100" (fn [] (sum-list (build-list 100 (list)) 0))]

   ["closures-100"
    (fn []
      (let [acc @[]]
        (each i in (range 100)
          (push acc (fn [y] (%add i y))))
        (freeze acc)))]

   ["struct-create-100"
    (fn []
      (let [acc @[]]
        (each i in (range 100)
          (push acc {:a i :b (%add i 1) :c (%add i 2)}))
        (freeze acc)))]

   ["struct-assoc-100"
    (fn []
      (def @s {:a 0 :b 0 :c 0})
      (each i in (range 100)
        (assign s (put s :a i)))
      s)]

   ["array-push-1000"
    (fn []
      (let [a @[]]
        (each i in (range 1000)
          (push a i))
        (length a)))]

   ["fiber-spawn-10"
    (fn []
      (each i in (range 10)
        (let [f (fiber/new (fn [] i) |:yield|)]
          (fiber/resume f))))]

   ["fiber-yield-100"
    (fn []
      (let [f (fiber/new (fn []
                           (each i in (range 100)
                             (yield i))) |:yield|)]
        (each _ in (range 100)
          (fiber/resume f))))]

   ["tco-loop-10000"
    (fn []
      (letrec [loop (fn [i] (if (= i 0) :done (loop (%sub i 1))))]
        (loop 10000)))]

   ["tco-alloc-10000"
    (fn []  # Per-parameter independence: {:a i :b (pair i nil)} does not
    # reference prev, so no cross-generation chain. Rotation safe.
    (letrec [loop (fn [i prev]
                    (if (= i 0)
                      prev
                      (loop (%sub i 1) {:a i :b (pair i nil)})))]
      (loop 10000 nil)))]

   ["tco-replace-10000"
    (fn []  # Struct replaced each iteration, no accumulation.
    # prev is overwritten, never referenced by the new struct.
    (letrec [loop (fn [i prev]
                    (if (= i 0)
                      prev
                      (loop (%sub i 1) {:x i :y (%add i 1)})))]
      (loop 10000 nil)))]

   ["tco-mixed-10000"
    (fn []  # Mixed: param 1 (prev) is replaced each iteration (rotation-safe),
    # param 2 (acc) accumulates via pair (rotation-unsafe because
    # (pair i acc) references acc).
    (letrec [loop (fn [i prev acc]
                    (if (= i 0) acc (loop (%sub i 1) {:x i} (pair i acc))))]
      (loop 10000 nil nil)))]

   ["let-no-escape"
    (fn []
      (letrec [loop (fn [i]
                      (if (= i 0)
                        :done
                        (let [a i
                              b (%add i 1)
                              c (%add i 2)]
                          (loop (%sub i 1)))))]
        (loop 100)))]

   ["let-drop-struct"
    (fn []  # Two struct bindings: a used in expr 0 only, b used in expr 1 only.
    # DropValue should fire for a after expr 0, for b after expr 1.
    (letrec [loop (fn [i]
                    (if (= i 0)
                      :done
                      (let [a {:x i}
                            b {:y (%add i 1)}]
                        (%add (a :x) (b :y))
                        (loop (%sub i 1)))))]
      (loop 100)))]

   ["tco-pair-replace"
    (fn []  # Each iteration replaces prev with a new pair cell.
    # DropValue + Cons fuses into ReuseSlotCons (in-place reuse).
    (letrec [loop (fn [i prev] (if (= i 0) prev (loop (%sub i 1) (pair i nil))))]
      (loop 10000 nil)))]

   ["string-build-100"
    (fn []
      (let [acc @[]]
        (each i in (range 100)
          (push acc (string "str-" i)))
        (length acc)))]

   ["keyword-build-20"
    (fn []  # Use string->keyword to create unique keywords at runtime
    (let [acc @[]]
      (each i in (range 20)
        (push acc (keyword (string "bench-kw-" i))))
      (length acc)))]])

# ── Run suite ─────────────────────────────────────────────────────

(println "# resource consumption benchmarks")
(println "# allocs=net heap objects  peak=high-water mark  bytes=heap bytes delta")
(println "# interns=new interned strings  symbols=new symbols  keywords=new keywords")
(def results (res:suite scenarios))

# ── Assertions (canaries) ─────────────────────────────────────────

(defn find-result [name]
  "Find measurement for a named scenario."
  (letrec [loop (fn [i]
                  (if (>= i (length results))
                    nil
                    (let [entry (results i)]
                      (if (= (entry 0) name) (entry 1) (loop (%add i 1))))))]
    (loop 0)))

# Under --checked-intrinsics, escape analysis sees Call instructions for
# %-intrinsics and sets outward_heap_set=true, disabling scope regions
# and flip rotation. Allocation bounds are only validated in default mode.

# TCO: net allocs and peak must be small — not proportional to iteration count
(let [m (find-result "tco-loop-10000")]
  (when (not checked?)
    (assert (%lt (m :allocs) 100)
            "tco-loop-10000: net allocs must be bounded (swap pool rotation working)")
    (assert (%lt (m :peak) 10)
            "tco-loop-10000: peak must be bounded (no per-iteration allocs)")))

# TCO with per-iteration struct + pair: rotation keeps this bounded at
# function-entry scope. tco-alloc replaces `prev` each iteration; the
# rotation pool frees the previous iteration's struct (and its inner
# pair) before the next iteration lives long enough to accumulate.
(let [m (find-result "tco-alloc-10000")]
  (when (not checked?)
    (assert (%lt (m :allocs) 10) "tco-alloc-10000: allocs bounded by rotation")))

# TCO replace: struct replaced each iteration, no sub-expression allocs.
# Rotation frees the prev struct → allocs and peak bounded.
(let [m (find-result "tco-replace-10000")]
  (when (not checked?)
    (assert (%lt (m :allocs) 10)
            "tco-replace-10000: allocs bounded (rotation working)")
    (assert (%lt (m :peak) 10)
            "tco-replace-10000: peak bounded (rotation working)")))

# TCO mixed: both `prev` and `acc` are replaced each iteration.
# `acc` aliases via `(pair i acc)` — trampoline rotation considered
# this unsafe, but function-level flip rotation (now on by default)
# resets alloc_count at each tail call, keeping net allocs bounded.
(let [m (find-result "tco-mixed-10000")]
  (when (not checked?)
    (assert (%lt (m :allocs) 100)
            "tco-mixed-10000: flip rotation keeps allocs bounded")))

# fib: pure arithmetic, no heap objects expected
(let [m (find-result "fib-15")]
  (when (not checked?)
    (assert (= (m :allocs) 0)
            "fib-15: pure arithmetic should allocate 0 heap objects")))

# pair-build-100: tail-recursive build-list gets flip rotation,
# so net allocs (visible_len delta) is bounded, not 100.
(let [m (find-result "pair-build-100")]
  (when (not checked?)
    (assert (%lt (m :allocs) 10)
            "pair-build-100: flip rotation keeps allocs bounded")))

# string-build-100: flip rotation resets alloc_count at each tail call,
# so net allocs may be 0 despite actual heap activity. Check peak instead.
(let [m (find-result "string-build-100")]
  (assert (> (m :peak) 0)
          "string-build-100: peak shows heap activity from string concatenation"))

# let-drop-struct: outer letrec loops 100 iters; each inner let allocates
# two structs. Escape analysis rejects scope allocation (the body reads
# from both a and b via callable struct syntax before the tail call),
# so allocs scale with iteration count (~2 per iter = 200 + overhead).
(let [m (find-result "let-drop-struct")]
  (when (not checked?)
    (assert (%lt (m :allocs) 300)
            "let-drop-struct: allocs bounded by 2 per iteration")))

# tco-pair-replace: rotation should keep allocs at minimum
(let [m (find-result "tco-pair-replace")]
  (when (not checked?)
    (assert (%lt (m :allocs) 10)
            "tco-pair-replace: allocs bounded (rotation working)")))

# All measurements should have non-negative allocs
(each entry in results
  (let [name (entry 0)
        m (entry 1)]
    (assert (>= (m :allocs) 0) (string name ": allocs must be non-negative"))))

(println "# all assertions passed")
