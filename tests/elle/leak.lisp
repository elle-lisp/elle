(elle/epoch 9)
# Tiered memory leak test suite
#
# Verifies that allocations are reclaimed at every level: scope,
# while-loop flip rotation, tail-call rotation, and yielding fibers.
# Each tier explicitly enables flip via vm/config to exercise the
# config path and make dependencies self-documenting.

# ── Tier 0: scope allocation ─────────────────────────────────────

(vm/config-set :flip :on)

(let [before (arena/count)]
  (def @i 0)
  (while (< i 10000)
    (let [x (cons i nil)] x)
    (assign i (+ i 1)))
  (let [delta (- (arena/count) before)]
    (assert (< delta 100)
      (string "tier 0: scope alloc leaked " delta))))

# ── Tier 1: while-loop flip rotation ─────────────────────────────

(vm/config-set :flip :on)

(defn tier1 []
  (def before (arena/count))
  (def @i 0)
  (while (< i 10000)
    (cons i nil)
    (assign i (+ i 1)))
  (- (arena/count) before))
(let [delta (tier1)]
  (assert (< delta 100)
    (string "tier 1: while flip leaked " delta)))

# ── Tier 2: tail-call rotation ───────────────────────────────────

(vm/config-set :flip :on)

(defn tier2 [n]
  (if (= n 0) (arena/count)
    (begin (cons n nil) (tier2 (- n 1)))))
(let* [before (arena/count)
       after (tier2 10000)
       delta (- after before)]
  (assert (< delta 100)
    (string "tier 2: tail-call leaked " delta)))

# ── Tier 3: yielding while loop ──────────────────────────────────

(vm/config-set :flip :on)

(let* [fiber (fiber/new
               (fn []
                 (def before (arena/count))
                 (def @i 0)
                 (while (< i 1000)
                   (cons i nil)
                   (yield i)
                   (assign i (+ i 1)))
                 (- (arena/count) before))
               |:yield|)
       @result 0]
  (while (not= (fiber/status fiber) :dead)
    (assign result (fiber/resume fiber)))
  (assert (< result 100)
    (string "tier 3: yielding while leaked " result)))
