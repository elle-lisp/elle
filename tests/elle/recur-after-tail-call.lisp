(elle/epoch 12)
## tests/elle/recur-after-tail-call.lisp
##
## A self-recursive tail loop replaces its own activation frame on every step
## (tail-call optimization): the call `(go …)` does not grow the stack, it
## re-enters `go`'s body in the same frame with new arguments. Across all those
## frame replacements the loop must keep recursing as ITSELF — the same body,
## carrying its own captured environment — for as many iterations as it runs.
##
## The hazard is silent: if the self-reference's identity were lost or went
## stale after a frame replacement, the loop would re-enter the wrong body or
## read a wrong captured value and return a plausible wrong number. No region
## leaks and no freed page is read, so neither the leak oracle nor
## `--trace=guardfree` would see it — only the value assertions below.

## 1. Deep accumulation: `step` is captured by `go` and re-read on every
## frame-replacing tail call. A loop that lost its captured environment across
## the replacement would accumulate the wrong amount. The depth (100000) makes
## the frame-replacement path the thing under test, not an incidental detail.
(defn deep-sum [n step]
  (letrec [go (fn [m acc] (if (%lt m 1) acc (go (%sub m 1) (%add acc step))))]
    (go n 0)))
(assert (= (deep-sum 100000 1) 100000)
        (concat "100000 tail steps of +1 must total 100000, got "
                (string (deep-sum 100000 1))))
(assert (= (deep-sum 50000 3) 150000)
        (concat "50000 tail steps of +3 must total 150000, got "
                (string (deep-sum 50000 3))))

## 2. Self-identity preserved to the base case. `go`'s base case returns `go`
## itself — a self-reference in value position reached only after every tail
## frame replacement of the descent. The returned closure must be the loop's own
## `go`, so invoking it once more (`(g 0)`) returns that same closure: a stale
## self-identity carried across the replacements would hand back the wrong value.
(defn descend-to-self [n]
  (letrec [go (fn [m] (if (%lt m 1) go (go (%sub m 1))))]
    (go n)))
(let [g (descend-to-self 100000)]
  (assert (= (g 0) g)
          "deep tail loop must preserve its own self-identity to the base case"))

## 3. Mixed: a tail loop whose accumulator is built from a captured seed string,
## interleaving heap allocation with the frame replacement, then asserted by
## length so the result depends on every step having run as the same closure.
## `seed` is a captured upvalue (the enclosing `let`), distinct from the
## self-edge, and must survive the frame replacement alongside it.
(defn deep-len [n]
  (let [seed "x"]
    (letrec [go (fn [m acc] (if (%lt m 1) acc (go (%sub m 1) (concat acc seed))))]
      (go n ""))))
(assert (= (length (deep-len 500)) 500)
        (concat "500 tail steps appending a captured seed must yield length 500, got "
                (string (length (deep-len 500)))))

(println "recur-after-tail-call: ok")
