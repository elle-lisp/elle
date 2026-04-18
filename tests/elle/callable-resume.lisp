(elle/epoch 8)
# Callable collection dispatch after fiber suspend/resume
#
# Regression test: (cell 0) callable @array syntax fails with
# "Cannot call @[...]" after repeated fiber park/resume cycles
# under concurrency. (get cell 0) works correctly in the same
# scenario. The bug is a stack misalignment in call_inner's
# suspend path (src/vm/call.rs).

# ── Helper: cell wrapper using callable syntax ─────────────────────────

(defn make-cell [initial]
  (let [cell @[initial]]
    {:get  (fn [] (cell 0))
     :set  (fn [v] (put cell 0 v))}))

# ── Helper: cooperative semaphore (no futex, just yield) ───────────────

(defn make-sem [n]
  (let [c (make-cell n)]
    {:acquire (fn []
       (while true
         (let [p (c:get)]
           (when (> p 0) (c:set (dec p)) (break nil))
           (ev/join (ev/spawn (fn [] nil))))))
     :release (fn []
       (c:set (inc (c:get)))
       nil)
     :permits (fn [] (c:get))}))

# ── 1. Single fiber: callable @array through cell wrapper ──────────────

(let [c (make-cell 42)]
  (assert (= 42 (c:get)) "1a: cell get works")
  (c:set 99)
  (assert (= 99 (c:get)) "1b: cell set/get works"))
(println "1: cell basic ok")

# ── 2. Callable @array inside fiber (no suspend) ──────────────────────

(let [c (make-cell 7)]
  (assert (= 7 (ev/join (ev/spawn (fn [] (c:get)))))
          "2a: cell get in fiber"))
(println "2: fiber no-suspend ok")

# ── 3. Callable @array after fiber yield ──────────────────────────────

(let [c (make-cell 0)]
  (let [f (ev/spawn (fn []
              (ev/join (ev/spawn (fn [] nil)))
              (c:get)))]
    (assert (= 0 (ev/join f)) "3a: cell get after yield")))
(println "3: post-yield ok")

# ── 4. Semaphore under concurrency (the failing case) ────────────────

(let [sem (make-sem 2)
      counter @[0]]
  (defn worker []
    (sem:acquire)
    (put counter 0 (inc (get counter 0)))
    (ev/join (ev/spawn (fn [] nil)))
    (put counter 0 (dec (get counter 0)))
    (sem:release))
  (ev/join (map (fn [_] (ev/spawn worker)) [1 2 3 4 5]))
  (assert (= 0 (get counter 0)) "4a: counter back to zero"))
(println "4: concurrent semaphore ok")

# ── 5. Nested callable dispatch after multiple resume cycles ──────────

(let [c (make-cell 0)]
  (defn bump []
    (c:set (inc (c:get))))
  (ev/join (map (fn [_]
    (ev/spawn (fn []
      (repeat 3
        (ev/join (ev/spawn (fn [] nil)))
        (bump)))))
    [1 2 3 4 5]))
  (assert (= 15 (c:get)) "5a: 5 fibers x 3 bumps = 15"))
(println "5: multi-resume bump ok")

(println "all callable-resume tests passed")
