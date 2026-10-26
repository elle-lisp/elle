(elle/epoch 15)
# audited: 2026-10-25
# Process schedulers side by side in one parent: each wakes for the completions forwarded to it.
# docs/process-scheduler.md
#
# Each case runs a ticker and one or more idlers, each in a `process:start`
# of its own, in fibers of the root scheduler. The ticker's process sleeps
# a short time, over and over. An idler's process sleeps once briefly, so
# that its scheduler forwards I/O and parks, and then sleeps past the end
# of the case.
#
# The counter-factual: every process scheduler parked on one shared key,
# and a delivery woke the scheduler that parked first. When that was an
# idler, the ticker stayed parked with its completion in its queue, and it
# woke only when a later delivery happened to pick it. The idlers' long
# sleeps are the only later deliveries, so the ticker froze. The defect
# depends on which scheduler parks first, so the cases cover both orders.
#
# A wait that the defect freezes is bounded by `ev/timeout`, so a red run
# fails rather than hangs.

(def process ((import "std/process")))

# The ticks a ticker runs, and the time one tick sleeps.
(def ticks 20)
(def tick 0.01)

# A deadline no correct run reaches: the ticker sleeps 0.2 s in all.
(def bound 10)

# Long enough for a fiber spawned before it to forward its I/O and park.
(def settle 0.05)

(defn ticker [counter]
  "A root fiber whose process scheduler runs one process that ticks
   `ticks` times. The process counts each tick in counter. Returns :done."
  (ev/spawn (fn []
              (process:start (fn []
                               (repeat ticks (ev/sleep tick)
                                       (put counter 0 (+ (get counter 0) 1)))))
              :done)))

(defn idler []
  "A root fiber whose process scheduler forwards one short sleep and then a
   long one, so that it stays parked for the rest of the case."
  (ev/spawn (fn []
              (process:start (fn []
                               (ev/sleep 0.001)
                               (ev/sleep 60))))))

(defn check-ticker [what t counter idlers]
  "Assert that ticker t ran every tick within the bound. Then abort the
   idlers, and assert that no fiber is left parked on a key."
  (assert (= (ev/timeout bound (fn [] (ev/join t))) :done)
          (string what
                  ": the ticker ends without waiting for another scheduler's sleep"))
  (assert (= (get counter 0) ticks) (string what ": the ticker runs every tick"))
  (each i in idlers
    (ev/abort i))
  (let [parks (get (ev/report) :parks)]
    (assert (empty? parks) (string what ": no park key is left, found " parks))))

# ── 1. The idler parks first ─────────────────────────────────────────

(println "a ticker started after an idler runs every tick...")

(let* [counter @[0]
       i (idler)]
  (ev/sleep settle)
  (check-ticker "idler first" (ticker counter) counter [i]))

# ── 2. The ticker parks first ────────────────────────────────────────

(println "a ticker started before an idler runs every tick...")

(let* [counter @[0]
       t (ticker counter)]
  (ev/sleep settle)
  (check-ticker "ticker first" t counter [(idler)]))

# ── 3. Two idlers, so that one delivery has two bystanders ───────────

(println "a ticker beside two idlers runs every tick...")

(let* [counter @[0]
       first-idler (idler)
       second-idler (idler)]
  (ev/sleep settle)
  (check-ticker "two idlers" (ticker counter) counter [first-idler second-idler]))

(println "process sibling schedulers: ok")
