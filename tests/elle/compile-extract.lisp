(elle/epoch 9)
# ── compile/extract tests ─────────────────────────────────────────────

# ── Basic extraction ─────────────────────────────────────────────────

(def src "
(defn process [data config]
  (let [x (+ data 1)]
    (let [y (* x config)]
      (+ y 10))))
")
(def a (compile/analyze src))

(def result (compile/extract a {:from :process :lines [4 4] :name :compute-y}))
(assert (not (nil? result)) "extract returned a result")
(assert (array? (get result :captures)) "captures is an array")
(assert (not (nil? (get result :new-function))) "new-function is present")
(assert (not (nil? (get result :source))) "source is present")
(assert (not (nil? (get result :signal))) "signal is present")

# The captures should include free variables used in the extracted range
(def caps (get result :captures))
(println "captures:" caps)

# Signal should be computed
(def sig (get result :signal))
# The let-destructure [y ...] is potentially erroring, so not fully silent
(assert (not (nil? sig)) "signal is present and computed")

# ── Extract with no captures ─────────────────────────────────────────

(def src2 "
(defn pure [x]
  (+ 1 2))
")
(def a2 (compile/analyze src2))
(def r2 (compile/extract a2 {:from :pure :lines [3 3] :name :constant}))
(assert (not (nil? r2)) "extract with no captures works")
(def caps2 (get r2 :captures))
(println "no-capture captures:" caps2)

# ── Error cases ──────────────────────────────────────────────────────

# Non-existent function
(let [[ok? _] (protect (compile/extract a {:from :nonexistent :lines [1 1] :name :x}))]
  (assert (not ok?) "extract from nonexistent function errors"))

# Invalid line range
(let [[ok? _] (protect (compile/extract a {:from :process :lines [5 2] :name :x}))]
  (assert (not ok?) "extract with invalid line range errors"))

(println "all compile/extract tests passed")
