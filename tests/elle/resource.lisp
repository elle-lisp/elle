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
      (let [[acc @[]]]
        (each i in (range 100)
          (push acc (fn [y] (+ i y))))
        (freeze acc)))]

   ["struct-create-100"
    (fn []
      (let [[acc @[]]]
        (each i in (range 100)
          (push acc {:a i :b (+ i 1) :c (+ i 2)}))
        (freeze acc)))]

   ["struct-assoc-100"
    (fn []
      (var s {:a 0 :b 0 :c 0})
      (each i in (range 100)
        (assign s (put s :a i)))
      s)]

   ["array-push-1000"
    (fn []
      (let [[a @[]]]
        (each i in (range 1000)
          (push a i))
        (length a)))]

   ["fiber-spawn-10"
    (fn []
      (each i in (range 10)
        (let [[f (fiber/new (fn [] i) |:yield|)]]
          (fiber/resume f))))]

   ["fiber-yield-100"
    (fn []
      (let [[f (fiber/new
                 (fn []
                   (each i in (range 100)
                     (yield i)))
                 |:yield|)]]
        (each _ in (range 100)
          (fiber/resume f))))]

   ["tco-loop-10000"
    (fn []
      (letrec [[loop (fn [i]
                  (if (= i 0) :done
                    (loop (- i 1))))]]
        (loop 10000)))]

   ["tco-alloc-10000"
    (fn []
      # Per-parameter independence: {:a i :b (cons i nil)} does not
      # reference prev, so no cross-generation chain. Rotation safe.
      (letrec [[loop (fn [i prev]
                  (if (= i 0) prev
                    (loop (- i 1) {:a i :b (cons i nil)})))]]
        (loop 10000 nil)))]

   ["tco-replace-10000"
    (fn []
      # Struct replaced each iteration, no accumulation.
      # prev is overwritten, never referenced by the new struct.
      (letrec [[loop (fn [i prev]
                  (if (= i 0) prev
                    (loop (- i 1) {:x i :y (+ i 1)})))]]
        (loop 10000 nil)))]

   ["tco-mixed-10000"
    (fn []
      # Mixed: param 1 (prev) is replaced each iteration (rotation-safe),
      # param 2 (acc) accumulates via cons (rotation-unsafe because
      # (cons i acc) references acc).
      (letrec [[loop (fn [i prev acc]
                  (if (= i 0) acc
                    (loop (- i 1) {:x i} (cons i acc))))]]
        (loop 10000 nil nil)))]

   ["let-no-escape"
    (fn []
      (letrec [[loop (fn [i]
                  (if (= i 0) :done
                    (let [[a i] [b (+ i 1)] [c (+ i 2)]]
                      (loop (- i 1)))))]]
        (loop 100)))]

   ["string-build-100"
    (fn []
      (let [[acc @[]]]
        (each i in (range 100)
          (push acc (string "str-" i)))
        (length acc)))]

   ["keyword-build-20"
    (fn []
      # Use string->keyword to create unique keywords at runtime
      (let [[acc @[]]]
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
  (letrec [[loop (fn [i]
              (if (>= i (length results)) nil
                (let [[entry (results i)]]
                  (if (= (entry 0) name)
                    (entry 1)
                    (loop (+ i 1))))))]]
    (loop 0)))

# TCO: net allocs and peak must be small — not proportional to iteration count
(let [[m (find-result "tco-loop-10000")]]
  (assert (< (m :allocs) 100)
    "tco-loop-10000: net allocs must be bounded (swap pool rotation working)")
  (assert (< (m :peak) 10)
    "tco-loop-10000: peak must be bounded (no per-iteration allocs)"))

# TCO with per-iteration allocation: per-parameter independence analysis
# (Perceus Phase 1) proves that {:a i :b (cons i nil)} does not reference
# the `prev` parameter, so no cross-generation reference chain exists.
# Swap pool rotation safely frees previous iteration's objects.
(let [[m (find-result "tco-alloc-10000")]]
  (assert (< (m :allocs) 10)
    "tco-alloc-10000: allocs bounded (rotation working)")
  (assert (< (m :peak) 10)
    "tco-alloc-10000: peak bounded (no accumulation)"))

# TCO replace: struct replaced each iteration, no accumulation.
(let [[m (find-result "tco-replace-10000")]]
  (assert (< (m :allocs) 10)
    "tco-replace-10000: allocs bounded (rotation working)")
  (assert (< (m :peak) 10)
    "tco-replace-10000: peak bounded (no accumulation)"))

# TCO mixed: prev is replaced (safe), acc accumulates via cons (unsafe).
# The function is rotation-unsafe because (cons i acc) references acc.
(let [[m (find-result "tco-mixed-10000")]]
  (assert (> (m :allocs) 10000)
    "tco-mixed-10000: allocs proportional to iterations (acc accumulates)"))

# fib: pure arithmetic, no heap objects expected
(let [[m (find-result "fib-15")]]
  (assert (= (m :allocs) 0)
    "fib-15: pure arithmetic should allocate 0 heap objects"))

# cons-build-100: exactly 100 cons cells
(let [[m (find-result "cons-build-100")]]
  (assert (= (m :allocs) 100)
    "cons-build-100: should allocate exactly 100 cons cells"))

# string-build-100: allocs should be positive (runtime strings go to heap, not interner)
(let [[m (find-result "string-build-100")]]
  (assert (> (m :allocs) 0)
    "string-build-100: should allocate heap objects for string concatenation"))

# All measurements should have non-negative allocs
(each entry in results
  (let [[name (entry 0)]
        [m    (entry 1)]]
    (assert (>= (m :allocs) 0)
      (string name ": allocs must be non-negative"))))

(println "# all assertions passed")
