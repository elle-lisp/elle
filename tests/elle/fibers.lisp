(elle/epoch 13)
# audited: 2026-09-28
# Fiber yield and resume: value order, signal masks, nested resumes, and the parent and child chain.
# tests/AGENTS.md
# docs/signals/fibers.md

## ── Yield and resume order ──────────────────────────────────────────

(let [f (fiber/new (fn []
                     (yield 1)
                     (yield 2)
                     3) 2)]
  (assert (= (fiber/resume f) 1) "yield order: first")
  (assert (= (fiber/resume f) 2) "yield order: second")
  (assert (= (fiber/resume f) 3) "yield order: final return"))

(let [f (fiber/new (fn []
                     (yield -50)
                     (yield 0)
                     (yield 50)
                     999) 2)]
  (assert (= (fiber/resume f) -50) "yield order: negative")
  (assert (= (fiber/resume f) 0) "yield order: zero")
  (assert (= (fiber/resume f) 50) "yield order: positive")
  (assert (= (fiber/resume f) 999) "yield order: final"))

(let [f (fiber/new (fn [] 42) 0)]
  (assert (= (fiber/resume f) 42) "fiber resume: a body that never yields"))

## ── The first resume value is the body's argument ───────────────────

(let [f (fiber/new (fn (s) (+ s 42)) 0)]
  (assert (= (fiber/resume f 8) 50) "fiber signal parameter: 8 + 42 = 50"))

(let [f (fiber/new (fn (s) (yield 42)) |:yield|)]
  (fiber/resume f 2)
  (assert (= (fiber/value f) 42) "fiber signal parameter: valid bits"))

(let [f (fiber/new (fn (x) (* x x)) 0)]
  (assert (= (fiber/resume f 7) 49)
          "fiber resume value as parameter: 7 * 7 = 49"))

## ── A yield crosses call frames ─────────────────────────────────────

(begin
  (def helper (fn [x] (yield (* x 2))))
  (def caller (fn [x] (+ (helper x) 1)))
  (def @co (fiber/new (fn [] (caller 5)) |:yield|))
  (assert (= (fiber/resume co) 10) "multi-frame yield: first yield is 5*2=10")
  (assert (= (fiber/resume co 7) 8)
          "multi-frame yield: resume 7, caller adds 1 = 8"))

(begin
  (def helper2 (fn [x] (yield (* x 2))))
  (def caller2 (fn [x] (+ (helper2 x) 1)))
  (def @co2 (fiber/new (fn [] (caller2 -25)) |:yield|))
  (assert (= (fiber/resume co2) -50)
          "multi-frame yield: first yield is -25*2=-50")
  (assert (= (fiber/resume co2 10) 11)
          "multi-frame yield: resume 10, caller adds 1 = 11"))

# A yield from a helper, then a yield from the body that called it.
(begin
  (def rh (fn [x] (yield x)))
  (def rgen
    (fn []
      (rh 10)
      (yield 20)
      42))
  (def @rco (fiber/new rgen |:yield|))
  (assert (= (fiber/resume rco) 10) "re-yield: first yield from helper")
  (assert (= (fiber/resume rco 5) 20) "re-yield: second yield from gen")
  (assert (= (fiber/resume rco) 42) "re-yield: final return"))

(begin
  (def rh2 (fn [x] (yield x)))
  (def rgen2
    (fn []
      (rh2 -30)
      (yield 50)
      99))
  (def @rco2 (fiber/new rgen2 |:yield|))
  (assert (= (fiber/resume rco2) -30) "re-yield: first yield -30")
  (assert (= (fiber/resume rco2 0) 50) "re-yield: second yield 50")
  (assert (= (fiber/resume rco2) 99) "re-yield: final return 99"))

# An error raised after the resume passes back through the suspended frames.
(begin
  (def eh
    (fn [x]
      (yield x)
      (/ 1 0)))
  (def egen (fn [] (+ (eh 5) 1)))
  (def @eco (fiber/new egen |:yield|))
  (fiber/resume eco)
  (let [[ok? _] (protect ((fn [] (fiber/resume eco))))]
    (assert (not ok?) "error during multi-frame resume: division by zero")))

(let* [f (fiber/new (fn []
                      (letrec [go (fn (n)
                                    (yield n)
                                    (go (+ n 1)))]
                        (go 0))) 2)]
  (assert (= (fiber/resume f) 0) "letrec binding: first yield")
  (assert (= (fiber/resume f) 1) "letrec binding: second yield")
  (assert (= (fiber/resume f) 2) "letrec binding: third yield"))

(begin
  (defn helper (n)
    (yield n)
    (helper (+ n 10)))
  (let* [f (fiber/new (fn [] (helper 1)) 2)]
    (assert (= (fiber/resume f) 1) "tail call signal: first")
    (assert (= (fiber/resume f) 11) "tail call signal: second")
    (assert (= (fiber/resume f) 21) "tail call signal: third")))

(begin
  (defn signaler (n)
    (yield n)
    (signaler (+ n 1)))
  (defn bouncer (n)
    (signaler n))
  (let* [f (fiber/new (fn [] (bouncer 100)) 2)]
    (assert (= (fiber/resume f) 100) "multiple tail calls: first")
    (assert (= (fiber/resume f) 101) "multiple tail calls: second")))

## ── Signal masks ────────────────────────────────────────────────────

# mask=2 catches SIG_YIELD (bit 2)
(let [f (fiber/new (fn [] (yield 42)) 2)]
  (assert (= (fiber/resume f) 42) "signal mask: yield caught by mask=2"))

# mask=0 does not catch SIG_YIELD, so the yield reaches the parent. The
# wrapper's mask=2 catches it there, which is how the test observes it.
(let [f (fiber/new (fn [] (yield 42)) 0)]
  (let [wrapper (fiber/new (fn [] (fiber/resume f)) 2)]
    (assert (= (fiber/resume wrapper) 42)
            "signal mask: uncaught yield propagates to parent")))

# mask=1 catches SIG_ERROR (bit 1)
(let [f (fiber/new (fn [] (emit :error 99)) 1)]
  (assert (= (fiber/resume f) 99) "signal mask: error caught by mask=1"))

# mask=0 does not catch SIG_ERROR; the wrapper's mask=1 catches it.
(let [f (fiber/new (fn [] (emit :error 99)) 0)]
  (let [wrapper (fiber/new (fn [] (fiber/resume f)) 1)]
    (assert (= (fiber/resume wrapper) 99)
            "signal mask: uncaught error propagates to parent")))

(let [[ok? _] (protect ((fn []
                          (let [f (fiber/new (fn [] (emit :error "oops")) 0)]
                            (fiber/resume f)))))]
  (assert (not ok?) "fiber error propagates without mask"))

# mask=3 catches both SIG_ERROR and SIG_YIELD
(let [f (fiber/new (fn [] (yield 77)) 3)]
  (assert (= (fiber/resume f) 77) "signal mask: yield caught by mask=3"))
(let [f (fiber/new (fn [] (emit :error 88)) 3)]
  (assert (= (fiber/resume f) 88) "signal mask: error caught by mask=3"))

# A caught error pauses the fiber; it does not end it.
(let [f (fiber/new (fn []
                     (emit :error "oops")
                     "recovered") 1)]
  (fiber/resume f)
  (assert (= (string (fiber/status f)) "paused")
          "caught SIG_ERROR: leaves fiber paused"))

(let [f (fiber/new (fn []
                     (emit :error "oops")
                     "recovered") 1)]
  (fiber/resume f)
  (assert (= (fiber/resume f) "recovered")
          "caught SIG_ERROR: fiber is resumable"))

## ── Nested resumes ──────────────────────────────────────────────────

(let [inner (fiber/new (fn [] (yield 10)) 2)]
  (let [outer (fiber/new (fn [] (+ (fiber/resume inner) 5)) 0)]
    (assert (= (fiber/resume outer) 15) "nested resume: 10 + 5 = 15")))

(let [inner (fiber/new (fn [] (yield -30)) 2)]
  (let [outer (fiber/new (fn [] (+ (fiber/resume inner) 20)) 0)]
    (assert (= (fiber/resume outer) -10) "nested resume: -30 + 20 = -10")))

(let [c (fiber/new (fn [] (yield 10)) 2)]
  (let [b (fiber/new (fn [] (+ (fiber/resume c) 5)) 0)]
    (let [a (fiber/new (fn [] (+ (fiber/resume b) 3)) 0)]
      (assert (= (fiber/resume a) 18) "3-level nested: 10 + 5 + 3 = 18"))))

(let [c (fiber/new (fn [] (yield -20)) 2)]
  (let [b (fiber/new (fn [] (+ (fiber/resume c) 30)) 0)]
    (let [a (fiber/new (fn [] (+ (fiber/resume b) -5)) 0)]
      (assert (= (fiber/resume a) 5) "3-level nested: -20 + 30 + -5 = 5"))))

(let [c (fiber/new (fn [] (emit :error "deep error")) 0)]
  (let [b (fiber/new (fn [] (fiber/resume c)) 0)]
    (let [a (fiber/new (fn [] (fiber/resume b)) 1)]
      (assert (= (fiber/resume a) "deep error")
              "3-level nested: the outermost mask catches the error"))))

(let [inner (fiber/new (fn [] 42) 0)]
  (let [outer (fiber/new (fn [] (fiber/resume inner)) 0)]
    (assert (= (fiber/resume outer) 42) "fiber resume: tail position")))

(let [inner (fiber/new (fn []
                         (yield 10)
                         20) 2)]
  (let [outer (fiber/new (fn [] (fiber/resume inner)) 0)]
    (assert (= (fiber/resume outer) 10) "fiber resume yield: tail position")))

## ── The parent and child chain ──────────────────────────────────────

(let [f (fiber/new (fn [] 42) 0)]
  (assert (= (fiber/child f) nil) "fiber child: nil before resume"))

(let [f (fiber/new (fn [] 42) 0)]
  (let [outer (fiber/new (fn []
                           (fiber/resume f)
                           42) 0)]
    (fiber/resume outer)
    (assert (= (identical? (fiber/parent f) (fiber/parent f)) true)
            "fiber parent: identity preserved")))

(let [inner (fiber/new (fn [] (emit :error "err")) 0)]
  (let [outer (fiber/new (fn []
                           (fiber/resume inner)
                           42) 1)]
    (fiber/resume outer)
    (assert (= (identical? (fiber/child outer) (fiber/child outer)) true)
            "fiber child: identity preserved")))

## ── A fiber outlives the scope that made it ─────────────────────────
##
## A fiber made inside a `let` and pushed into an array from outside it
## escapes the scope. An escape analysis that sees only `assign` misses the
## push and frees the fiber at scope exit. The resumes below then read a
## freed slot: `fiber/bits` reports the wrong type, or the process crashes.

(begin
  (def @fibers @[])
  (let [f (fiber/new (fn []
                       (yield :ping)
                       :done) 2)]
    (push fibers f))
  (assert (fiber? (get fibers 0))
          "a fiber survives its let scope after a push into an outer array")
  (assert (= (fiber/resume (get fibers 0)) :ping)
          "a fiber stored in an outer array is still resumable"))

(begin
  (def @store @[])
  (let [a (fiber/new (fn [] 1) 0)]
    (push store a))
  (let [b (fiber/new (fn [] 2) 0)]
    (push store b))
  (assert (= (fiber/resume (get store 0)) 1)
          "the first fiber from a separate let scope is valid")
  (assert (= (fiber/resume (get store 1)) 2)
          "the second fiber from a separate let scope is valid"))

(begin
  (def @bucket @[])
  (let [@pid 0
        f (fiber/new (fn []
                       (yield :hello)
                       :world) 3)]
    (push bucket f)
    (assign pid (length bucket)))
  (let [f (get bucket 0)
        result (fiber/resume (get bucket 0))]
    (assert (= result :hello)
            "a fiber pushed from a nested let: first resume value")
    (assert (= (fiber/bits f) 2)
            "a fiber pushed from a nested let: fiber/bits is the yield bit")))
