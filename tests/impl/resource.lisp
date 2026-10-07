(elle/epoch 14)
# audited: 2026-10-05
# Resource consumption across representative scenarios: each one's net heap
# objects and its high-water mark, read against the rows of
# tests/ledger/resource.lisp.
# docs/ratchet.md
#
# Uses lib/resource.lisp to measure deterministic resource counters. The
# suite prints its table for a reader; the ratchet reads two numbers per
# scenario out of it.

(def res ((import-file "lib/resource.lisp")))
(def r ((import "std/ratchet")))
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
    (let [x (first lst)]
      # The guard proves x for the silent %add (build-list only makes ints).
      (when (%not (%int? x))
        (error {:error :type-error :message "sum-list: int expected"}))
      (sum-list (rest lst) (%add acc x)))))

# ── Scenarios ─────────────────────────────────────────────────────

(def scenarios
  [["fib-15" (fn [] (fib 15))]

   ["pair-build-100" (fn [] (build-list 100 (list)))]

   ["pair-sum-100" (fn [] (sum-list (build-list 100 (list)) 0))]

   ["closures-100"
    (fn []
      (let [acc @[]]
        (each i in (range 100)
          # The guard proves both the captured element and the (never-called)
          # closure's param for %add.
          (push acc (fn [y] (if (and (%int? i) (%int? y)) (%add i y) i))))
        (freeze acc)))]

   ["struct-create-100"
    (fn []
      (let [acc @[]]
        (each i0 in (range 100)
          # Allocation-free coerce-guard: proves the element for %add without
          # perturbing the measured per-iteration allocation profile.
          (let [i (if (%int? i0) i0 0)]
            (push acc {:a i :b (%add i 1) :c (%add i 2)})))
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
    (fn []
      # Per-parameter independence: {:a i :b (pair i nil)} does not
      # reference prev, so no cross-generation chain. Rotation safe.
      (letrec [loop (fn [i prev]
                      (if (= i 0)
                        prev
                        (loop (%sub i 1) {:a i :b (pair i nil)})))]
        (loop 10000 nil)))]

   ["tco-replace-10000"
    (fn []
      # Struct replaced each iteration, no accumulation.
      # prev is overwritten, never referenced by the new struct.
      (letrec [loop (fn [i prev]
                      (if (= i 0)
                        prev
                        (loop (%sub i 1) {:x i :y (%add i 1)})))]
        (loop 10000 nil)))]

   ["tco-mixed-10000"
    (fn []
      # Mixed: param 1 (prev) is replaced each iteration (rotation-safe),
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
    (fn []
      # Two struct bindings: a used in expr 0 only, b used in expr 1 only.
      # DropValue should fire for a after expr 0, for b after expr 1.
      (letrec [loop (fn [i]
                      (if (= i 0)
                        :done
                        (let [a {:x i}
                              b {:y (%add i 1)}]
                          # Both structs are read before the tail call (what
                          # forces the two DropValues); the coerce-guards
                          # prove the reads for %add without allocating.
                          (let [ax (a :x)
                                by (b :y)]
                            (%add (if (%int? ax) ax 0) (if (%int? by) by 0)))
                          (loop (%sub i 1)))))]
        (loop 100)))]

   ["tco-pair-replace"
    (fn []
      # Each iteration replaces prev with a new pair cell.
      # DropValue + Cons fuses into ReuseSlotCons (in-place reuse).
      (letrec [loop (fn [i prev]
                      (if (= i 0) prev (loop (%sub i 1) (pair i nil))))]
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

# ── The readings ──────────────────────────────────────────────────
# The byte delta is printed and not read: a page's size is the platform's.

(each entry in results
  (let [name (entry 0)
        m (entry 1)]
    (r:read name :allocs (m :allocs))
    (r:read name :peak (m :peak))))

(println "# all scenarios read")
