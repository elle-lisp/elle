(elle/epoch 12)
# audited: 2026-09-23
# A process's exit drops every futex park it owns, so a later futex wake counts and reaches only live waiters.
# docs/process-scheduler.md

(def process ((import "std/process")))

(defn wake-after-kill [park-doomed count]
  "Park a doomed process with park-doomed, and a live process on the key :k.
   Kill the doomed one, then wake count waiters on :k. Returns a struct:
   :woken is the count the wake reported, and :reply is what the live
   process sent.

   The live process is killed before anything is asserted, so a failed
   assertion never leaves it parked."
  (def @result nil)
  (process:start (fn []
                   (let* [me (process:self)
                          bx (box 0)
                          doomed (process:spawn (fn [] (park-doomed bx)))
                          live (process:spawn (fn []
                            (ev/futex-wait :k bx 0)
                            (process:send me :woken)))]
                     (process:recv-timeout 2)
                     (process:exit doomed :kill)
                     (let* [woken (ev/futex-wake :k count)
                            reply (process:recv-timeout 10)]
                       (process:exit live :kill)
                       (assign result {:woken woken :reply reply})))))
  result)

# ── a process parked on a futex is killed ────────────────────────────
# The counter-factual: the dead process's park stayed on the key. The wake
# spent its count on it, reported one waiter woken, and the live waiter
# stayed parked.

(defn park-self [bx]
  (ev/futex-wait :k bx 0))

(let [got (wake-after-kill park-self 1)]
  (assert (= got:woken 1) "the wake takes one waiter")
  (assert (= got:reply :woken) "and that waiter is the live process"))

# A wake for more waiters than remain counts only the live ones.
(let [got (wake-after-kill park-self 2)]
  (assert (= got:woken 1) "the wake counts the live waiter alone")
  (assert (= got:reply :woken) "and wakes it"))

# ── a sub-fiber of a killed process is parked ────────────────────────

(defn park-sub-fiber [bx]
  (ev/spawn (fn [] (ev/futex-wait :k bx 0)))
  (process:recv))

(let [got (wake-after-kill park-sub-fiber 1)]
  (assert (= got:woken 1) "the wake takes one waiter")
  (assert (= got:reply :woken) "and that waiter is the live process"))

(println "process-futex: ok")
