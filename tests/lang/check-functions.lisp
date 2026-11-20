(elle/epoch 18)
# audited: 2026-11-20
# std/check: generated functions, whose calls draw their answers from the case, and the calls a failing run reports.
# docs/stdlib/check/functions.md
#
# The counter-factual: a function that drew its answers from a stream of its
# own still fails a property, but a replay of the case's bits cannot repeat
# it, and the shrinker cannot lower its answers. So each test here names the
# smallest answer, and the calls that gave it.

(def check ((import "std/check")))
(include-file "check/parts.lisp")

(def digit (check:function (check:int 0 9)))

# ── what a generated function answers ─────────────────────────────────
(let [src (source:replay [0 1 0 1 0 0 1 1])
      f (digit (src :draw))]
  (assert (empty? (((src :record)) :choices)) "making a function draws nothing")
  (let [a (f :x)
        b (f :x)
        c (f 1 2)]
    (assert (= [5 3 0] [a b c])
            "each call draws, so equal arguments can answer different values")
    (assert (= 3 (length (((src :record)) :choices)))
            "and each call is a draw of the case")
    (assert (= [[[[:x] 5] [[:x] 3] [[1 2] 0]]] (((src :record)) :calls))
            "and the source's record holds the calls")))

(let [f (digit (fn [n &opt edges] 4))]
  (assert (= [4 4] [(f) (f :y)])
          "a function made with a draw no source made answers from that draw"))

(let [[f] (check:enumerate (check:function (check:int 3 7)) 0)]
  (assert (= [3 3] [(f) (f :y)])
          "the simplest function answers the simplest value on every call"))

(assert (= 1 (length (check:enumerate digit 4)))
        "the enumeration calls no function, so one function is one case")
(assert (= [0 1 2]
           (map (fn [[f x]] x)
                (check:enumerate (check:tuple digit (check:int 0 9)) 2)))
        "and a tuple with a function enumerates its other parts")

# ── run walks the answers, and shrinks them ───────────────────────────
(def bools (check:function (check:bool)))
(defn not-twice [f]
  (not (and (f) (f))))

(let [r (check:run bools not-twice)]
  (assert (= false (r :passed)) "a property that calls a function can fail")
  (assert (= 3 (r :cases)) "the enumeration reaches the second answer")
  (assert (= [[[[] true] [[] true]]] (r :calls))
          ":calls holds each call's arguments and answer")
  (assert (= [1 1] (r :choices)) "one draw for each call")
  (let [f (r :value)]
    (assert (= false (f)) "a call after the case reads zeros past its bits")
    (assert (= 2 (length (get (r :calls) 0))) "and :calls leaves it out")))

(def pair (check:tuple (check:function (check:int 0 100)) (check:int 0 100)))
(defn under-50 [[f x]]
  (< (f x) 50))

(each seed in (range 5)
  (let [r (check:run pair under-50 :depth 0 :seed seed)]
    (assert (= [[[[0] 50]]] (r :calls))
            (string "seed " seed ": the argument and the answer shrink"))
    (assert (= [0 50] (r :choices)) "the generator's draw, then the call's")
    (let [again (check:run pair under-50 :bits (r :bits))]
      (assert (= (r :calls) (again :calls)) "a replay of :bits makes the calls")
      (assert (= 0 ((get (again :value) 0) 7))
              "and a call after a replayed case answers the simplest value"))))

(def calls-under-50
  (check:tuple (check:function (check:int 0 100)) (check:int 0 5)))
(defn all-under-50 [[f n]]
  (all? (fn [i] (< (f i) 50)) (range n)))

(each seed in (range 5)
  (let [r (check:run calls-under-50 all-under-50 :depth 0 :seed seed)]
    (assert (= [[[[0] 50]]] (r :calls))
            (string "seed " seed ": the calls in front of the failure go"))
    (assert (= 1 (get (r :value) 1)) "so one call is enough")))

# ── which functions :calls holds ──────────────────────────────────────
(let [two (check:tuple digit bools)
      r (check:run two (fn [[f g]] (not (and (g) (> (f) 0)))))]
  (assert (= [[[[] 1]] [[[] true]]] (r :calls))
          "in the order the case made them, not the order of their calls"))

(let [r (check:run (check:tuple digit bools) (fn [[f g]] (not (g))))]
  (assert (= [[] [[[] true]]] (r :calls))
          "a function the case never called has no calls"))

# The counter-factual: a repeated draw that handed its elements a draw
# without the source's trait would leave these two functions out.
(let [r (check:run (check:array-of bools 2 :min 2)
                   (fn [[f g]] (not (and (f) (g)))))]
  (assert (= [[[[] true]] [[[] true]]] (r :calls))
          "a function made as an element of an array logs its calls"))

(let [r (check:run (check:function bools) (fn [h] (not ((h)))))]
  (assert (= 2 (length (r :calls)))
          "a function a call answers is a function the case made")
  (assert (= [[[] true]] (get (r :calls) 1)) "with the calls made to it"))

(assert (= [] (get (check:run (check:int 0 9) (fn [n] (< n 5))) :calls))
        "a case that made no function has no calls")
(assert (nil? (get (check:run digit (fn [f] (< (f) 10))) :calls))
        "and a run that passed holds none")

# ── a call that raises or skips ───────────────────────────────────────
(def raising
  (check:function (fn [draw] (error {:error :bad-answer :message "no answer"}))))

(let [r (check:run raising
                   (fn [f]
                     (f)
                     true))]
  (assert (= :property (r :failed-in)) "a call in the property fails there")
  (assert (= :bad-answer (get (r :error) :error)) "with what the call raised"))

(let [r (check:run (check:map (fn [f] (f)) raising) (fn [_] true))]
  (assert (= :generator (r :failed-in))
          "a call in the generator fails in the generator"))

(let [odd (check:function (check:filter (check:int 0 9) odd?))
      r (check:run odd
                   (fn [f]
                     (f)
                     false) :bits [0 0 0 0])]
  (assert (= true (r :passed)) "a call whose answer the filter rejects skips")
  (assert (= [0 1] [(r :cases) (r :skipped)]) "the case"))

# ── assert names the calls ────────────────────────────────────────────
(defn message-of [g prop]
  "The message of the error `check:assert` raises."
  (let [[ok? err] (protect (check:assert g prop))]
    (assert (not ok?) "the assertion fails")
    (err :message)))

(assert (string/contains? (message-of pair under-50)
                          (string (get (check:run pair under-50) :calls)))
        "the message names the calls of the shrunk case")
(assert (not (string/contains? (message-of (check:int 0 99) (fn [n] (< n 50)))
                               "[]"))
        "and names none for a case that made no function")

# ── the submodules compose as the module does ─────────────────────────
(assert (= [[[[] true]]]
           (get (run:run (gen:function (gen:bool)) (fn [f] (not (f)))) :calls))
        "the record of the case holds the calls")

(println "check-functions: all tests passed")
