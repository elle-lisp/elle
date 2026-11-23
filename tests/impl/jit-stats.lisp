(elle/epoch 18)
## audited: 2026-11-23
## jit/stats counts the JIT compiles one VM took, refused ones included, with their wall and CPU time.
## docs/impl/jit.md
##
## The counter-factuals: two process-wide counters that the background worker
## alone fed. A compile `compile/run-on :jit` runs on the VM's own thread
## counted nowhere, and a fresh VM read what every other VM had compiled.

(def jit-on? (not (nil? (vm/config :jit))))

(defn counts []
  "This VM's JIT counts, as [compiles wall-us cpu-us]."
  (let [s (jit/stats)]
    [(get s :compiles) (get s :wall-us) (get s :cpu-us)]))

(eprintln "scenario: jit/stats answers three counts")
(let [s (jit/stats)]
  (assert (struct? s) "jit/stats answers a struct")
  (each k in [:compiles :wall-us :cpu-us]
    (assert (integer? (get s k))
            (string "jit/stats answers " k " as an integer"))
    (assert (>= (get s k) 0) (string k " is never negative"))))

(when (not jit-on?)
  (eprintln "scenario: a build without the JIT counts nothing")
  (assert (= (counts) [0 0 0]) "a build without the JIT compiles nothing"))

# No call grows hot at this threshold, so the forced compiles below are the
# only compiles this VM takes until the last scenario lowers it. The trap: the
# eager profile submits a compile for each call before this line, and its
# result would land between two readings. `jit/rejections` waits for every
# compile in flight.
(when jit-on?
  (vm/config-set :jit 1000000)
  (jit/rejections))

(when jit-on?
  (eprintln "scenario: a forced compile counts once, on both clocks")
  (let [[c0 w0 u0] (counts)
        f (fn [x] (+ x 1))]
    (assert (= (compile/run-on :jit f 1) 2) "the compiled closure runs")
    (let [[c1 w1 u1] (counts)]
      (assert (= c1 (+ c0 1)) "compile/run-on :jit counts its compile")
      (assert (> w1 w0) "the compile adds wall time")
      (assert (> u1 u0) "the compile adds CPU time"))
    (compile/run-on :jit f 1)
    (assert (= (get (counts) 0) (+ c0 1))
            "a closure with code in the cache compiles no more")))

(when jit-on?
  (eprintln "scenario: a refused compile counts")
  (let [c0 (get (counts) 0)
        [ok? err] (protect (compile/run-on :jit (fn [] (eval '1))))]
    (assert (not ok?) "the JIT refuses a closure that calls eval")
    (assert (= (get err :error) :tier-rejected)
            "the refusal is a tier rejection")
    (assert (= (get (counts) 0) (+ c0 1)) "the refused compile counts")))

(when jit-on?
  (eprintln "scenario: a fresh VM counts its own compiles alone")
  (assert (> (get (counts) 0) 0) "this VM has compiled")
  (assert (= (os/join (os/spawn-vm (fn [] (get (jit/stats) :compiles)))) 0)
          "a fresh VM has compiled nothing, whatever another VM compiled"))

# The calls poll the worker, and the loop ends once the VM has taken the
# result: `jit?` reads the cache alone. The trap: the graft splices the body of
# a closure the compiler can prove into each call, so an immutable `h` is never
# called and never grows hot. A mutable binding proves nothing.
(when jit-on?
  (eprintln "scenario: a background compile counts once the VM takes it")
  (vm/config-set :jit 1)
  (let [[c0 w0 u0] (counts)
        @h (fn [x] (* x 2))
        @n 0]
    (while (and (not (jit? h)) (< n 2000))
      (h n)
      (time/sleep 0.001)
      (assign n (+ n 1)))
    (assert (jit? h) "the worker compiled the hot closure")
    (let [[c1 w1 u1] (counts)]
      (assert (> c1 c0) "the worker's compile counts")
      (assert (> w1 w0) "the worker's compile adds wall time")
      (assert (> u1 u0) "the worker's compile adds the CPU time of its thread"))))

(eprintln "all jit-stats scenarios passed")
