(elle/epoch 18)
## audited: 2026-11-23
## The rig's jit/stats counts the JIT compiles one VM took, the rejected ones apart, with their wall and CPU time.
## docs/impl/jit.md
## rig/overview.md
##
## The counter-factuals: two process-wide counters that the background worker
## alone fed. A compile `compile/run-on :jit` runs on the VM's own thread
## counted nowhere, and a fresh VM read what every other VM had compiled. A
## count of compiles alone cannot say how many produced code.
##
## The sidecar keeps every call a call. A graft splices the body of the hot
## closure into each call, so the closure is never called and never compiles.

(def jit-on? (not (nil? (vm/config :jit))))

(defn counts []
  "This VM's JIT counts, as [compiles rejected wall-us cpu-us]."
  (let [s (jit/stats)]
    [(get s :compiles) (get s :rejected) (get s :wall-us) (get s :cpu-us)]))

(eprintln "scenario: jit/stats answers four counts")
(let [s (jit/stats)]
  (assert (struct? s) "jit/stats answers a struct")
  (each k in [:compiles :rejected :wall-us :cpu-us]
    (assert (integer? (get s k))
            (string "jit/stats answers " k " as an integer"))
    (assert (>= (get s k) 0) (string k " is never negative")))
  (assert (<= (get s :rejected) (get s :compiles))
          "no more compiles are rejected than were taken"))

# jit/stats polls the compile worker, and a user build carries no poll.
(eprintln "scenario: elle has no jit/stats")
(let [elle (path/join (path/parent (elle/executable)) "elle")
      r (subprocess/system elle ["-e" "(jit/stats)"])]
  (assert (not (= r:exit 0))
          (string "a user build must not answer (jit/stats); stdout: " r:stdout))
  (assert (string/contains? r:stderr "jit/stats")
          (string "the refusal names the missing primitive: " r:stderr)))

(when (not jit-on?)
  (eprintln "scenario: a build without the JIT counts nothing")
  (assert (= (counts) [0 0 0 0]) "a build without the JIT compiles nothing"))

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
  (let [[c0 r0 w0 u0] (counts)
        f (fn [x] (+ x 1))]
    (assert (= (compile/run-on :jit f 1) 2) "the compiled closure runs")
    (let [[c1 r1 w1 u1] (counts)]
      (assert (= c1 (+ c0 1)) "compile/run-on :jit counts its compile")
      (assert (= r1 r0) "a compile that produced code is no rejection")
      (assert (> w1 w0) "the compile adds wall time")
      (assert (> u1 u0) "the compile adds CPU time"))
    (compile/run-on :jit f 1)
    (assert (= (get (counts) 0) (+ c0 1))
            "a closure with code in the cache compiles no more")))

(when jit-on?
  (eprintln "scenario: a refused compile counts, and counts as rejected")
  (let [[c0 r0 _ _] (counts)
        [ok? err] (protect (compile/run-on :jit (fn [] (eval '1))))]
    (assert (not ok?) "the JIT refuses a closure that calls eval")
    (assert (= (get err :error) :tier-rejected)
            "the refusal is a tier rejection")
    (let [[c1 r1 _ _] (counts)]
      (assert (= c1 (+ c0 1)) "the refused compile counts")
      (assert (= r1 (+ r0 1)) "the refused compile counts as rejected"))))

(when jit-on?
  (eprintln "scenario: a fresh VM counts its own compiles alone")
  (assert (> (get (counts) 0) 0) "this VM has compiled")
  (assert (> (get (counts) 1) 0) "this VM has rejected a compile")
  (assert (= (os/join (os/spawn-vm (fn []
                                     (let [s (jit/stats)]
                                       [s:compiles s:rejected])))) [0 0])
          "a fresh VM has compiled and rejected nothing, whatever another VM did"))

# The calls poll the worker, and the loop ends once the VM has taken the
# result: `jit?` reads the cache alone.
(when jit-on?
  (eprintln "scenario: a background compile counts once the VM takes it")
  (vm/config-set :jit 1)
  (let [[c0 _ w0 u0] (counts)
        h (fn [x] (* x 2))
        @n 0]
    (while (and (not (jit? h)) (< n 2000))
      (h n)
      (time/sleep 0.001)
      (assign n (+ n 1)))
    (assert (jit? h) "the worker compiled the hot closure")
    (let [[c1 _ w1 u1] (counts)]
      (assert (> c1 c0) "the worker's compile counts")
      (assert (> w1 w0) "the worker's compile adds wall time")
      (assert (> u1 u0) "the worker's compile adds the CPU time of its thread"))))

(eprintln "all jit-stats scenarios passed")
