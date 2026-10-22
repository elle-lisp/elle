(elle/epoch 15)
# audited: 2026-10-21
# std/gun states and the HAM verdict: the hybrid logical clock, its ticks and observations, and which write wins.
# lib/gun.md

(include-file "gun/check.lisp")

(def gun ((import "std/gun")))
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))
(def ham ((import "std/gun/ham") :state state :value value))

# ============================================================================
# States
# ============================================================================

(let [c (frozen-clock 1000)
      a (c:tick)
      b (c:tick)
      d (c:tick)]
  (assert (= a (state:pack 1000 0)) "the first tick is the wall clock")
  (assert (state:before? a b) "a tick follows the tick before it")
  (assert (state:before? b d) "and the one after follows it")
  (assert (= (state:unpack d) [1000 2])
          "a frozen clock counts within its millisecond"))

(let [c (frozen-clock 1000)]
  (c:observe (state:pack 5000 7))
  (assert (= (c:tick) (state:pack 5000 8)) "a tick follows an observed state")
  (c:observe (state:pack 10 0))
  (assert (= (c:tick) (state:pack 5000 9))
          "observing an older state moves nothing"))

(let [@ms 1000
      c (state:make-clock :now-ms (fn [] ms))]
  (c:tick)
  (assign ms 2000)
  (assert (= (c:tick) (state:pack 2000 0))
          "a later wall clock restarts the counter"))

(let [c (frozen-clock 1000)]
  (assert (= (c:now) (- (state:pack 1001 0) 1))
          "now is the last state of the current millisecond")
  (c:observe (state:pack 9000 3))
  (assert (= (c:now) (state:pack 9000 3))
          "now never runs behind an observed state"))

(let [c (frozen-clock 1000)]
  (assert (= (c:due-in (state:pack 1000 9)) 0)
          "a state of this millisecond is due")
  (assert (= (c:due-in (state:pack 900 0)) 0) "a past state is due")
  (assert (= (c:due-in (state:pack 1250 0)) 0.25)
          "a later state is due when the wall clock reaches its millisecond")
  (c:observe (state:pack 3000 0))
  (assert (= (c:due-in (state:pack 2000 0)) 0)
          "a state no later than an observed one is due"))

(assert (= (state:unpack (state:pack 1791766440908 65535)) [1791766440908 65535])
        "pack and unpack round-trip")
(assert (= (state:parse (state:format (state:pack 77 3))) (state:pack 77 3))
        "format and parse round-trip")
(assert (not (state:before? (state:pack 5 0) (state:pack 5 0)))
        "a state is not before itself")
(assert (state:before? nil (state:pack 0 0)) "no state is before every state")
(assert (not (state:before? (state:pack 0 0) nil))
        "and nothing is before no state")

# ============================================================================
# The HAM verdict
# ============================================================================

(assert (= (ham:verdict 10 5 "a" nil nil) :accept) "a first write is accepted")
(assert (= (ham:verdict 10 6 "a" 5 "z") :accept) "a later state is accepted")
(assert (= (ham:verdict 10 4 "z" 5 "a") :stale) "an earlier state is stale")
(assert (= (ham:verdict 10 11 "z" 5 "a") :future) "a state after now is future")
(assert (= (ham:verdict 10 11 "z" nil nil) :future)
        "future even with no current field")
(assert (= (ham:verdict 10 10 "z" nil nil) :accept)
        "a state equal to now is not future")
(assert (= (ham:verdict 10 5 "a" 5 "a") :same) "an equal write is same")

# The tiebreak, in both argument orders. A rule that let the incoming value
# win every tie would accept both of these, and two replicas that saw the
# writes in different orders would keep different values.
(assert (= (ham:verdict 10 5 "b" 5 "a") :accept)
        "on a tie the later encoding wins")
(assert (= (ham:verdict 10 5 "a" 5 "b") :same) "and the earlier encoding loses")
(assert (= (ham:verdict 10 5 nil 5 "a") :same)
        "a tombstone does not win a tie it sorts before")
(assert (= (gun:ham 10 5 "b" 5 "a") :accept) "the module exports the verdict")

(all-passed "gun-state")
