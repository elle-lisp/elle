(elle/epoch 15)
# audited: 2026-11-17
# A compiled self tail call binds its arguments as the prologue does: a fresh cell per celled parameter, a fresh list per rest.
# docs/impl/jit.md
#
# Each function here tail-calls itself, so with the JIT eager its compiled body
# runs the self call as a native loop. The loop must bind the new arguments the
# way the prologue binds a call's. A parameter that the body assigns lives in a
# cell, and a rest parameter holds a list of the arguments past the fixed ones.
#
# The counter-factual: the loop wrote each new argument straight into its
# parameter. A celled parameter then held a raw value, and each read through
# its cell printed `JIT type error: expected capture cell` and answered nil. A
# rest parameter held the raw last argument, where the interpreter holds a list
# of it.
#
# The gauge reads the move. A tail call moves its arguments, and a rest list
# takes its own reference to each one it collects, so the moved reference is
# surplus and the loop releases it. A loop that kept it leaves one value per
# trip, and the delta reads about the depth.
#
# The sidecar compiles each function on the VM thread at its first call, and
# turns the graft off, so each tail call stays a call.

(defn sum-down [@n acc]
  (assign n (- n 1))
  (if (< n 0) acc (sum-down n (+ acc n))))

(defn grow [n @acc]
  (assign acc (concat acc "x"))
  (if (= n 0) acc (grow (- n 1) acc)))

(defn opt-sum [@n &opt acc]
  (assign n (- n 1))
  (if (< n 0)
    acc
    (opt-sum n (+ (or acc 0) n))))

(defn last-rest [n & r]
  (if (= n 0) r (last-rest (- n 1) 5)))

(defn rest-len [n & r]
  (if (= n 0) (length (first r)) (rest-len (- n 1) (bytes "abcdef"))))

(defn celled-rest [@n & r]
  (assign n (- n 1))
  (if (< n 0) r (celled-rest n n)))

(defn twice [n b & r]
  (if (= n 0)
    (+ (length b) (length (first r)))
    (let [v (bytes "abcdef")]
      (twice (- n 1) v v))))

# Repeat each call, so the answer is read from the compiled body whatever tier
# ran the first one.
(defn each-time [thunk expected label]
  (repeat 20
    (let [got (thunk)]
      (assert (= got expected)
              (string label ": expected " expected ", got " got)))))

(each-time (fn [] (sum-down 10 0)) 45 "a celled counter")
(each-time (fn [] (grow 3 "")) "xxxx" "a celled heap value")
(each-time (fn [] (opt-sum 10)) 45 "a celled parameter beside an optional one")
(each-time (fn [] (last-rest 3 1)) (list 5) "a rest list")
(each-time (fn [] (rest-len 3 (bytes "x"))) 6 "a rest list of a heap value")
(each-time (fn [] (celled-rest 3 9)) (list 0)
           "a celled parameter beside a rest list")
(each-time (fn [] (twice 3 (bytes "a") (bytes "b"))) 12
           "one value as a fixed argument and as a rest argument")

# The frame parks after two trips through the loop, and the interpreter
# resumes it, reading the celled parameter the parked frame holds.
(defn countdown [@n acc]
  (assign n (- n 1))
  (cond
    (< n 0) acc
    (= n 1)
      (begin
        (yield n)
        (countdown n (+ acc n)))
    (countdown n (+ acc n))))

(repeat 20
  (let [gen (fiber/new (fn [] (countdown 4 0)) |:yield|)]
    (assert (= (fiber/resume gen) 1) "the frame yields the celled value")
    (assert (= (fiber/resume gen) 6) "the resumed frame finishes the loop")))

(when (vm/config :jit)
  (each f in [sum-down grow opt-sum last-rest rest-len celled-rest twice
              countdown]
    (assert (jit? f) (string "precondition: " f " compiles"))))

(def depth 10000)

(defn measure [start]
  "Run `start` `depth` trips deep after a warm-up, and answer how many objects
  it left behind."
  (start 3)
  (start 3)
  (def before (arena/count))
  (assert (= (start depth) 6) "the loop runs to its end")
  (- (arena/count) before))

(def d (measure (fn [n] (rest-len n (bytes "x")))))
(assert (< d 60) (string "a rest list keeps its moved argument, delta=" d))

(println "jit-self-tail-bind: ok")
