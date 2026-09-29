(elle/epoch 12)
# Region inference diagnostic tests
#
# Progressively narrows down why the Tofte-Talpin region inference
# fails to classify let bindings inside tail-recursive functions as
# Scope regions.


(defn bounded? [d100 d10k limit]
  (and (%lt d100 limit) (%lt d10k limit) (or (= d100 0) (%lt d10k (* d100 10)))))

(defn report [name ok msg]
  (if ok (println "  PASS " name) (println "  FAIL " name ": " msg)))

# t1: Simple let with string init, integer result
(let [before (arena/count)
      _ (let [s (string "x")]
          42)
      d (%sub (arena/count) before)]
  (report "t1-simple-let" (%lt d 2) (string "delta=" d)))

# t2: Let with string init inside a named function
(defn t2-fn []
  (let [s (string "x")]
    42))
(let [before (arena/count)
      _ (t2-fn)
      d (%sub (arena/count) before)]
  (report "t2-fn-let" (%lt d 2) (string "delta=" d)))

# t3: Let with string init, tail call to other function returning int
(defn return-count []
  (arena/count))
(defn t3-fn []
  (let [s (string "x")]
    (return-count)))
(let [before (arena/count)
      _ (t3-fn)
      d (%sub (arena/count) before)]
  (report "t3-let-tailcall" (%lt d 2) (string "delta=" d)))

# t4: Let with string init, direct arena/count tail call
(defn t4-fn []
  (let [s (string "x")]
    (arena/count)))
(let [before (arena/count)
      _ (t4-fn)
      d (%sub (arena/count) before)]
  (report "t4-let-tailcall-inline" (%lt d 2) (string "delta=" d)))

# t5: Self tail recursion with string alloc (THE FAILING CASE)
(defn t5-loop (n)
  (if (%le n 0)
    (arena/count)
    (let* [s (string "iter-" n)]
      (t5-loop (%sub n 1)))))

(let* [c100 (t5-loop 100)
       c10k (t5-loop 10000)]
  (report "t5-tail-loop" (%lt c10k (%mul c100 10))
          (string "c100=" c100 " c10k=" c10k)))

# t6: Self tail recursion with string in begin (not let)
(defn t6-loop (n)
  (if (= n 0)
    (arena/count)
    (begin
      (string "iter-" n)
      (t6-loop (%sub n 1)))))

(let* [b1 (arena/count)
       a1 (t6-loop 100)
       d100 (%sub a1 b1)
       b2 (arena/count)
       a2 (t6-loop 10000)
       d10k (%sub a2 b2)]
  (report "t6-tail-begin" (bounded? d100 d10k 10)
          (string "d100=" d100 " d10k=" d10k)))

# t7: Self tail recursion with integer let (baseline, no heap alloc)
(defn t7-loop (n)
  (if (%le n 0)
    (arena/count)
    (let [x (%add n 1)]
      (t7-loop (%sub n 1)))))

(let* [c1 (t7-loop 100)
       c2 (t7-loop 10000)]
  (report "t7-int-loop" (%lt c2 (%mul c1 10)) (string "c1=" c1 " c2=" c2)))

# t8: While loop with string let (proven working path)
(defn t8-while (n)
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [s (string "iter-" i)]
      s)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-while 100)
      d10k (t8-while 10000)]
  (report "t8-while-let" (bounded? d100 d10k 10)
          (string "d100=" d100 " d10k=" d10k)))

# t9: Non-recursive function with let + string (single call)
(defn t9-fn []
  (let [s (string "hello")]
    (length s)))
(let [before (arena/count)
      _ (t9-fn)
      d (%sub (arena/count) before)]
  (report "t9-fn-single-let" (%lt d 2) (string "delta=" d)))

# t10: Two lets in sequence inside a function
(defn t10-fn []
  (let [a (string "a")]
    (let [b (string "b")]
      (%add (length a) (length b)))))
(let [before (arena/count)
      _ (t10-fn)
      d (%sub (arena/count) before)]
  (report "t10-two-lets" (%lt d 2) (string "delta=" d)))

# t11: Self tail recursion with struct alloc (not string)
(defn t11-loop (n)
  (if (= n 0)
    (arena/count)
    (begin
      {:x n}
      (t11-loop (%sub n 1)))))

(let* [b1 (arena/count)
       a1 (t11-loop 100)
       d100 (%sub a1 b1)
       b2 (arena/count)
       a2 (t11-loop 10000)
       d10k (%sub a2 b2)]
  (report "t11-tail-struct" (bounded? d100 d10k 10)
          (string "d100=" d100 " d10k=" d10k)))

# t12: Self tail recursion with let-bound struct
(defn t12-loop (n)
  (if (= n 0)
    (arena/count)
    (let [s {:x n}]
      (t12-loop (%sub n 1)))))

(let* [c100 (t12-loop 100)
       c10k (t12-loop 10000)]
  (report "t12-tail-let-struct" (%lt c10k (%mul c100 10))
          (string "c100=" c100 " c10k=" c10k)))

(println "region-scope: done")

# t13: Let with fiber yield inside (may suspend) — should NOT scope-allocate
# If the region inference incorrectly allows scope allocation for suspending
# code, RegionExit fires while fiber is suspended, freeing values still in use.
(let [f (fiber/new (fn []
                     (let [s (string "hello")]
                       (yield (length s))
                       (length s))) |:yield|)]
  (let [v1 (fiber/resume f)]
    (report "t13-yield-let-resume1" (= v1 5) (string "v1=" v1))
    (let [v2 (fiber/resume f)]
      (report "t13-yield-let-resume2" (= v2 5) (string "v2=" v2)))))

# t14: Let inside a function called from a yielding fiber
(let [f (fiber/new (fn []
                     (let [s (string "inner")]
                       (yield (length s))
                       (length s))) |:yield|)]
  (let [v1 (fiber/resume f)]
    (report "t14-fiber-inner-let-resume1" (= v1 5) (string "v1=" v1))
    (let [v2 (fiber/resume f)]
      (report "t14-fiber-inner-let-resume2" (= v2 5) (string "v2=" v2)))))

# t15: ev/spawn pattern (like grpc's with-server)
(let [sf (ev/spawn (fn []
                     (let [s (string "spawned")]
                       (length s))))]
  (let [result (ev/join sf)]
    (report "t15-ev-spawn" (= result 7) (string "result=" result))))

(println "region-scope: done (with fiber tests)")
