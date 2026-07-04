(elle/epoch 12)
# Basic region tests: verify that scope allocation and release work
# correctly with the region infrastructure.

# Simple let scope: allocations inside should be reclaimable
(let [x "hello"
      y "world"]
  (assert (= x "hello"))
  (assert (= y "world")))

# Nested let scopes
(let [outer "outer"]
  (let [inner "inner"]
    (assert (= inner "inner")))
  (assert (= outer "outer")))

# Loop with string allocation per iteration
(def @count 0)
(while (< count 5)
  (let [s (string count)]
    (assert (string? s)))
  (assign count (+ count 1)))
(assert (= count 5))

# Yield through a fiber: the yielded value must survive child death
(let [f (fiber/new (fn []
                     (yield "from-child")
                     42) |:yield|)]
  (let [result (fiber/resume f)]
    (assert (= result "from-child"))
    (let [final (fiber/resume f)]
      (assert (= final 42)))))

# Yield a struct through a fiber
(let [f (fiber/new (fn [] (yield {:name "alice" :age 30})) |:yield|)]
  (let [result (fiber/resume f)]
    (assert (= (get result :name) "alice"))
    (assert (= (get result :age) 30))))

# Yield an array through a fiber
(let [f (fiber/new (fn [] (yield [1 2 3])) |:yield|)]
  (let [result (fiber/resume f)]
    (assert (= (get result 0) 1))
    (assert (= (get result 2) 3))))

:ok
