(elle/epoch 12)
## tests/elle/recur-after-yield.lisp
##
## A self-recursive local function that suspends mid-recursion must, on resume,
## continue recursing as ITSELF — same body, same captured environment — even
## while other fibers running the SAME-shaped recursion are interleaved between
## its resumes.
##
## Each `make-summer` builds a generator whose body is a self-recursive `go`
## that captures its own `base`, yields between steps, and accumulates
## `base` four times. Driving three such generators round-robin (a, b, c) means
## that between any two resumes of one generator the runtime has switched fibers
## and run a DIFFERENT generator's `go`. The self-reference's identity is
## per-activation: when `a` resumes it must re-enter `a`'s `go` with `base` 10,
## not the `go`/`base` of whichever generator ran most recently.
##
## The hazard this pins is silent: if the resumed self-reference were the wrong
## closure (the most-recently-active `go` instead of this fiber's own), the sums
## would simply come out wrong — no region leaks, no use-after-free, so neither
## the leak oracle nor `--trace=guardfree` would notice. Only these value
## assertions catch it. The three distinct `base`s (10, 100, 1000) make any
## cross-wiring between the fibers produce a detectably wrong total.

## go 4 0 yields at m = 4,3,2,1 (four yields) then returns 4*base at m = 0, so
## each generator needs exactly five resumes: four ticks and one final value.
(defn make-summer [base]
  (fiber/new (fn []
               (letrec [go (fn [m acc]
                             (if (%lt m 1)
                               acc
                               (begin
                                 (yield :tick)
                                 (go (%sub m 1) (%add acc base)))))]
                 (go 4 0))) |:yield|))

(def a (make-summer 10))
(def b (make-summer 100))
(def c (make-summer 1000))

## Strictly interleave: each round resumes a, then b, then c, so a fiber switch
## sits between every pair of a single generator's resumes. After five rounds
## each variable holds that generator's final accumulated sum.
(def @fa nil)
(def @fb nil)
(def @fc nil)
(def @r 0)
(while (%lt r 5)
  (assign fa (fiber/resume a))
  (assign fb (fiber/resume b))
  (assign fc (fiber/resume c))
  (assign r (%add r 1)))

(assert (= fa 40) (concat "summer base=10 must total 40, got " (string fa)))
(assert (= fb 400) (concat "summer base=100 must total 400, got " (string fb)))
(assert (= fc 4000)
        (concat "summer base=1000 must total 4000, got " (string fc)))

## A self-recursive generator interleaved with the driver's OWN heap churn: the
## driver allocates between resumes (the region/object churn a real scheduler
## pump produces), and the generator must still recurse over its own captured
## state across each suspend.
(defn make-collector [tag]
  (fiber/new (fn []
               (letrec [go (fn [m acc]
                             (if (%lt m 1)
                               acc
                               (begin
                                 (yield :tick)
                                 (go (%sub m 1) (concat acc tag)))))]
                 (go 3 ""))) |:yield|))
(let [g (make-collector "ab")]
  (def @last nil)
  (while (not= (fiber/status g) :dead)
    (assign last (fiber/resume g))  ## churn the driver's regions between resumes
    (let [junk {:k (%pair last last)}]
      (get junk :k)))
  (assert (= last "ababab")
          (concat "collector must accumulate its captured tag, got "
                  (string last))))

(println "recur-after-yield: ok")
