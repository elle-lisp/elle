(elle/epoch 15)
# audited: 2026-11-18
# Each stdlib function that builds a closure compiles once hot, and answers on a list and an array as the interpreter does.
# docs/impl/jit-closure.md
#
# Each of these functions builds a walker or a predicate closure in one of its
# arms, so each holds a `MakeClosure`. The sidecar makes the JIT eager and
# turns the graft off, so every call below reaches the stdlib function itself.
# Each case reaches it with the collection through a parameter, sixty times on
# an array and sixty on a list, and `(jit/rejections)` then waits for the
# compiles in flight.
#
# The counter-factual is a JIT that refuses `MakeClosure`: every function here
# runs interpreted, `(jit/rejections)` holds one `MakeClosure` refusal for
# each, and the answers stay right, so only the rejections and `jit?` tell.
#
# In a build without the JIT `(vm/config :jit)` reads nil, nothing compiles,
# and the answers alone are checked.

(def jit-on? (not (nil? (vm/config :jit))))

(def arr [1 2 3 4 5 6])
(def lst (list 1 2 3 4 5 6))

(defn spread [f c]
  (f ;c))

(defn twice [c]
  (if (array? c) (fn [x] [x x]) (fn [x] (list x x))))

# Each case: the function, a call of it on a collection, and its answers on
# `arr` and on `lst`.
(def cases
  [[any? (fn [c] (any? (fn [x] (> x 5)) c)) true true]
   [all? (fn [c] (all? (fn [x] (> x 0)) c)) true true]
   [filter (fn [c] (filter (fn [x] (> x 3)) c)) [4 5 6] (list 4 5 6)]
   [find (fn [c] (find (fn [x] (> x 2)) c)) 3 3]
   [find-index (fn [c] (find-index (fn [x] (> x 2)) c)) 2 2]
   [count (fn [c] (count (fn [x] (> x 2)) c)) 4 4]
   [map-indexed (fn [c] (map-indexed (fn [i x] (+ i x)) c)) [1 3 5 7 9 11]
    (list 1 3 5 7 9 11)]
   [mapcat (fn [c] (mapcat (twice c) c)) [1 1 2 2 3 3 4 4 5 5 6 6]
    (list 1 1 2 2 3 3 4 4 5 5 6 6)]
   [take-while (fn [c] (take-while (fn [x] (< x 3)) c)) [1 2] (list 1 2)]
   [drop-while (fn [c] (drop-while (fn [x] (< x 3)) c)) [3 4 5 6] (list 3 4 5 6)]
   [take (fn [c] (take 2 c)) [1 2] (list 1 2)]
   [drop (fn [c] (drop 2 c)) [3 4 5 6] (list 3 4 5 6)]
   [flatten (fn [c] (flatten c)) [1 2 3 4 5 6] (list 1 2 3 4 5 6)]
   [distinct (fn [c] (distinct c)) [1 2 3 4 5 6] (list 1 2 3 4 5 6)]
   [partition (fn [c] (partition 2 c)) [[1 2] [3 4] [5 6]]
    (list (list 1 2) (list 3 4) (list 5 6))]
   [interpose (fn [c] (interpose 0 c)) [1 0 2 0 3 0 4 0 5 0 6]
    (list 1 0 2 0 3 0 4 0 5 0 6)]
   [zip (fn [c] (zip c c))
    [(list 1 1) (list 2 2) (list 3 3) (list 4 4) (list 5 5) (list 6 6)]
    (list (list 1 1) (list 2 2) (list 3 3) (list 4 4) (list 5 5) (list 6 6))]
   [min (fn [c] (spread min c)) 1 1] [+ (fn [c] (spread + c)) 21 21]
   [trait/elements (fn [c] (trait/elements c)) [1 2 3 4 5 6] [1 2 3 4 5 6]]])

(each [f call on-arr on-lst] cases
  (repeat 60
    (call arr)
    (call lst)))
(def rejections (jit/rejections))

(each [f call on-arr on-lst] cases
  (assert (= (call arr) on-arr) (string f ": the answer on an array"))
  (assert (= (call lst) on-lst) (string f ": the answer on a list")))

(each r rejections
  (assert (not (string/contains? r:reason "MakeClosure"))
          (string "the JIT refused " r:name " for building a closure")))

(when jit-on?
  (each [f call on-arr on-lst] cases
    (assert (jit? f) (string f " compiles once hot"))))

(println "jit-stdlib-walks: ok")
