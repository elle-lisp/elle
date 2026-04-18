(elle/epoch 8)
# Resource consumption measurement tests
#
# Uses lib/resource.lisp to measure deterministic resource counters
# across representative scenarios. Output is machine-parseable for CI
# regression detection.

(def res ((import-file "lib/resource.lisp")))

# ── Helper definitions ────────────────────────────────────────────

(defn fib [n]
  (if (< n 2) n
    (+ (fib (- n 1)) (fib (- n 2)))))

(defn build-list [n acc]
  (if (= n 0) acc
    (build-list (- n 1) (cons n acc))))

(defn sum-list [lst acc]
  (if (empty? lst) acc
    (sum-list (rest lst) (+ acc (first lst)))))

# ── Scenarios ─────────────────────────────────────────────────────

(def scenarios
  [["fib-15"
    (fn [] (fib 15))]

   ["cons-build-100"
    (fn [] (build-list 100 (list)))]

   ["cons-sum-100"
    (fn [] (sum-list (build-list 100 (list)) 0))]

   ["closures-100"
    (fn []
      (let [acc @[]]
        (each i in (range 100)
          (push acc (fn [y] (+ i y))))
        (freeze acc)))]

   ["struct-create-100"
    (fn []
      (let [acc @[]]
        (each i in (range 100)
          (push acc {:a i :b (+ i 1) :c (+ i 2)}))
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
      (let [f (fiber/new
                 (fn []
                   (each i in (range 100)
                     (yield i)))
                 |:yield|)]
        (each _ in (range 100)
          (fiber/resume f))))]

   ["tco-loop-10000"
    (fn []
      (letrec [loop (fn [i]
                  (if (= i 0) :done
                    (loop (- i 1))))]
        (loop 10000)))]

   ["tco-alloc-10000"
    (fn []
      # Per-parameter independence: {:a i :b (cons i nil)} does not
      # reference prev, so no cross-generation chain. Rotation safe.
      (letrec [loop (fn [i prev]
                  (if (= i 0) prev
                    (loop (- i 1) {:a i :b (cons i nil)})))]
        (loop 10000 nil)))]

   ["tco-replace-10000"
    (fn []
      # Struct replaced each iteration, no accumulation.
      # prev is overwritten, never referenced by the new struct.
      (letrec [loop (fn [i prev]
                  (if (= i 0) prev
                    (loop (- i 1) {:x i :y (+ i 1)})))]
        (loop 10000 nil)))]

   ["tco-mixed-10000"
    (fn []
      # Mixed: param 1 (prev) is replaced each iteration (rotation-safe),
      # param 2 (acc) accumulates via cons (rotation-unsafe because
      # (cons i acc) references acc).
      (letrec [loop (fn [i prev acc]
                  (if (= i 0) acc
                    (loop (- i 1) {:x i} (cons i acc))))]
        (loop 10000 nil nil)))]

   ["let-no-escape"
    (fn []
      (letrec [loop (fn [i]
                  (if (= i 0) :done
                    (let [a i b (+ i 1) c (+ i 2)]
                      (loop (- i 1)))))]
        (loop 100)))]

   ["let-drop-struct"
    (fn []
      # Two struct bindings: a used in expr 0 only, b used in expr 1 only.
      # DropValue should fire for a after expr 0, for b after expr 1.
      (letrec [loop (fn [i]
                  (if (= i 0) :done
                    (let [a {:x i}
                          b {:y (+ i 1)}]
                      (+ (a :x) (b :y))
                      (loop (- i 1)))))]
        (loop 100)))]

   ["tco-cons-replace"
    (fn []
      # Each iteration replaces prev with a new cons cell.
      # DropValue + Cons fuses into ReuseSlotCons (in-place reuse).
      (letrec [loop (fn [i prev]
                  (if (= i 0) prev
                    (loop (- i 1) (cons i nil))))]
        (loop 10000 nil)))]

   ["string-build-100"
    (fn []
      (let [acc @[]]
        (each i in (range 100)
          (push acc (string "str-" i)))
        (length acc)))]

   ["keyword-build-20"
    (fn []
      # Use string->keyword to create unique keywords at runtime
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
              (if (>= i (length results)) nil
                (let [entry (results i)]
                  (if (= (entry 0) name)
                    (entry 1)
                    (loop (+ i 1))))))]
    (loop 0)))

# TCO: net allocs and peak must be small — not proportional to iteration count
(let [m (find-result "tco-loop-10000")]
  (assert (< (m :allocs) 100)
    "tco-loop-10000: net allocs must be bounded (swap pool rotation working)")
  (assert (< (m :peak) 10)
    "tco-loop-10000: peak must be bounded (no per-iteration allocs)"))

# TCO with per-iteration struct + cons: rotation keeps this bounded at
# function-entry scope. tco-alloc replaces `prev` each iteration; the
# rotation pool frees the previous iteration's struct (and its inner
# cons) before the next iteration lives long enough to accumulate.
(let [m (find-result "tco-alloc-10000")]
  (assert (< (m :allocs) 10)
    "tco-alloc-10000: allocs bounded by rotation"))

# TCO replace: struct replaced each iteration, no sub-expression allocs.
# Rotation frees the prev struct → allocs and peak bounded.
(let [m (find-result "tco-replace-10000")]
  (assert (< (m :allocs) 10)
    "tco-replace-10000: allocs bounded (rotation working)")
  (assert (< (m :peak) 10)
    "tco-replace-10000: peak bounded (rotation working)"))

# TCO mixed: both `prev` and `acc` are replaced each iteration, but
# `acc` aliases via `(cons i acc)` — escape analysis classifies this
# call as rotation-unsafe, so both params' allocations accumulate.
# Allocs ~ 2 * 10000 (one struct + one cons per iteration).
(let [m (find-result "tco-mixed-10000")]
  (assert (> (m :allocs) 10000)
    "tco-mixed-10000: cons + struct both accumulate (rotation-unsafe)")
  (assert (< (m :allocs) 20100)
    "tco-mixed-10000: accumulation is bounded to two per iteration"))

# fib: pure arithmetic, no heap objects expected
(let [m (find-result "fib-15")]
  (assert (= (m :allocs) 0)
    "fib-15: pure arithmetic should allocate 0 heap objects"))

# cons-build-100: exactly 100 cons cells
(let [m (find-result "cons-build-100")]
  (assert (= (m :allocs) 100)
    "cons-build-100: should allocate exactly 100 cons cells"))

# string-build-100: allocs should be positive (runtime strings go to heap, not interner)
(let [m (find-result "string-build-100")]
  (assert (> (m :allocs) 0)
    "string-build-100: should allocate heap objects for string concatenation"))

# let-drop-struct: outer letrec loops 100 iters; each inner let allocates
# two structs. Escape analysis rejects scope allocation (the body reads
# from both a and b via callable struct syntax before the tail call),
# so allocs scale with iteration count (~2 per iter = 200 + overhead).
(let [m (find-result "let-drop-struct")]
  (assert (< (m :allocs) 300)
    "let-drop-struct: allocs bounded by 2 per iteration"))

# tco-cons-replace: rotation should keep allocs at minimum
(let [m (find-result "tco-cons-replace")]
  (assert (< (m :allocs) 10)
    "tco-cons-replace: allocs bounded (rotation working)"))

# All measurements should have non-negative allocs
(each entry in results
  (let [name (entry 0)
        m    (entry 1)]
    (assert (>= (m :allocs) 0)
      (string name ": allocs must be non-negative"))))

(println "# all assertions passed")
