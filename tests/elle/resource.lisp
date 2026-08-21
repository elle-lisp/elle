(elle/epoch 12)
# Resource consumption measurement tests
#
# Uses lib/resource.lisp to measure deterministic resource counters
# across representative scenarios. Output is machine-parseable for CI
# regression detection.

(def res ((import-file "lib/resource.lisp")))

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
                          # Both structs are read before the tail call (what
                          # forces the two DropValues); the coerce-guards
                          # prove the reads for %add without allocating.
                          (let [ax (a :x)
                                by (b :y)]
                            (%add (if (%int? ax) ax 0) (if (%int? by) by 0)))
                          (loop (%sub i 1)))))]
        (loop 100)))]

   ["tco-pair-replace"
    (fn []  # Each iteration replaces prev with a new pair cell.
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

# ── Assertions (canaries) ─────────────────────────────────────────

(defn find-result [name]
  "Find measurement for a named scenario."
  (letrec [loop (fn [i]
                  (if (>= i (length results))
                    nil
                    (let [entry (results i)]
                      (if (= (entry 0) name) (entry 1) (loop (%add i 1))))))]
    (loop 0)))

# TCO: net allocs and peak must be small — not proportional to iteration count
(let [m (find-result "tco-loop-10000")]
  (assert (< (m :allocs) 100)
          "tco-loop-10000: net allocs must be bounded (swap pool rotation working)")
  (assert (< (m :peak) 10)
          "tco-loop-10000: peak must be bounded (no per-iteration allocs)"))

# TCO with a per-iteration struct EMBEDDING a pair: both the struct and its
# inner pair are retained to teardown (~2N) — the loop-scope over-keep for a
# param-threaded aggregate with a heap member (the F1-class scratch retain;
# contrast tco-replace below, whose member-free struct rotates at ~0).
# CANARY, shrink-only: a fix lowers this pin.
(let [m (find-result "tco-alloc-10000")]
  (assert (< (m :allocs) 20100)
          "tco-alloc-10000: struct + inner pair retained (~2/iter, canary)"))

# TCO replace: a fresh struct threaded as the tail-call arg each iteration;
# every displaced prior is retained to scope exit (~1/iter) — the
# param-threaded tail-arg over-keep (the F1-class scratch retain).
# CANARY, shrink-only: a fix lowers this pin.
(let [m (find-result "tco-replace-10000")]
  (assert (< (m :allocs) 10100)
          "tco-replace-10000: displaced prior structs retained (~1/iter, canary)"))

# TCO mixed: `acc` grows via (pair i acc) — all pairs are live (the result
# is a 10000-element linked list, N genuine allocs) — and the per-iteration
# `prev` struct rides the same param-threaded tail-arg over-keep as
# tco-replace (~1/iter more). CANARY on the struct half, shrink-only.
(let [m (find-result "tco-mixed-10000")]
  (assert (< (m :allocs) 20100)
          "tco-mixed-10000: live pair chain O(N) + displaced structs (canary)"))

# fib: pure arithmetic, no heap objects expected
(let [m (find-result "fib-15")]
  (assert (= (m :allocs) 0)
          "fib-15: pure arithmetic should allocate 0 heap objects"))

# pair-build-100: builds a 100-element linked list via tail recursion.
# All 100 pairs are live in the return value — allocs = N.
(let [m (find-result "pair-build-100")]
  (assert (= (m :allocs) 100) "pair-build-100: 100 live pairs in result"))

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
  (assert (< (m :allocs) 300)
          "let-drop-struct: allocs bounded by 2 per iteration"))

# tco-pair-replace: the same param-threaded tail-arg over-keep as
# tco-replace, with a pair cell (~1/iter). CANARY, shrink-only.
(let [m (find-result "tco-pair-replace")]
  (assert (< (m :allocs) 10100)
          "tco-pair-replace: displaced prior pairs retained (~1/iter, canary)"))

# All measurements should have non-negative allocs
(each entry in results
  (let [name (entry 0)
        m (entry 1)]
    (assert (>= (m :allocs) 0) (string name ": allocs must be non-negative"))))

(println "# all assertions passed")
