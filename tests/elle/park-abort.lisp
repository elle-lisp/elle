(elle/epoch 12)
# Park queues and fiber termination.
#
# `ev/futex-wait` parks a fiber on a key; `ev/futex-wake` wakes up to
# `count` of the fibers parked on that key. A fiber can also be killed
# while it is parked — `ev/abort` does it directly, and `ev/timeout`
# does it whenever the deadline beats the body.
#
# Two invariants hold across that, and both are pinned here:
#
#   1. Only live fibers wait. A fiber that reached :dead or :error is in
#      no park queue, so it cannot take a wake slot from a live waiter.
#   2. A wake reaches a live waiter. `(ev/futex-wake key 1)` grants one
#      permit; it must land on a fiber that can use it.
#
# The single-permit wake is the shape that makes this matter: a channel
# put wakes one taker (lib/http2/stream.lisp), and a SETTINGS ACK wakes
# one waiter (lib/http2/session.lisp). One permit spent on a dead fiber
# is a permit the live waiter never receives, and it parks forever.
#
# Every join here runs under a deadline. A lost wake is a hang, and a
# hang inside a test reports nothing; the deadline turns it into a value
# the assertions can name.

# The deadline every join gets. Two orders of magnitude above what these
# fibers cost, so only a lost wake can reach it.
(def deadline 5)

(defn join-by-deadline [f]
  "Join `f`, or return :timed-out when it does not finish in time."
  (let [r (ev/timeout deadline (fn [] (ev/join f)))]
    (if (nil? r) :timed-out r)))

# ── 1. A fiber aborted while parked leaves the queue ─────────────────

(println "abort removes a parked fiber from its queue...")

(let [key (gensym)
      bx (box 0)
      f (ev/spawn (fn []
                    (ev/futex-wait key bx 0)
                    :woke))]
  (ev/sleep 0.05)
  (ev/abort f)
  # The only waiter is gone, so this permit finds nobody to grant.
  (assert (= 0 (ev/futex-wake key 1))
          "a wake after the only waiter was aborted must reach nobody"))

# ── 2. A timed-out park leaves the queue ─────────────────────────────

(println "ev/timeout removes the parked fiber it kills...")

(let [key (gensym)
      bx (box 0)]
  # Nothing ever wakes this key, so the deadline wins and kills the
  # parked body.
  (assert (nil? (ev/timeout 0.05 (fn [] (ev/futex-wait key bx 0))))
          "ev/timeout must fire on a park nothing wakes")
  (assert (= 0 (ev/futex-wake key 1))
          "a wake after the parked body timed out must reach nobody"))

# ── 3. A dead fiber does not eat a live waiter's wake ────────────────

(println "a killed waiter does not consume a later permit...")

(let [key (gensym)
      bx (box 0)]
  (assert (nil? (ev/timeout 0.05 (fn [] (ev/futex-wait key bx 0))))
          "ev/timeout must fire on a park nothing wakes")
  # Same key, live waiter, one permit. It must land here.
  (let [waiter (ev/spawn (fn []
                           (ev/futex-wait key bx 0)
                           :woke))]
    (ev/sleep 0.05)
    (assert (= 1 (ev/futex-wake key 1)) "the permit must be granted")
    (assert (= :woke (join-by-deadline waiter))
            "the live waiter must receive the permit")))

# ── 4. Repeated kills do not fill the queue ──────────────────────────

(println "repeated timed-out parks leave the queue clean...")

(let [key (gensym)
      bx (box 0)]
  (each i in (range 0 20)
    (assert (nil? (ev/timeout 0.02 (fn [] (ev/futex-wait key bx 0))))
            (string "ev/timeout must fire on park " i)))
  (let [waiter (ev/spawn (fn []
                           (ev/futex-wait key bx 0)
                           :woke))]
    (ev/sleep 0.05)
    # One permit still reaches the one live waiter, whatever came before.
    (assert (= 1 (ev/futex-wake key 1))
            "the permit must be granted after 20 killed parks")
    (assert (= :woke (join-by-deadline waiter))
            "the live waiter must receive the permit after 20 killed parks")))

# ── 5. The channel shape that depends on this ────────────────────────
#
# `lib/http2/stream.lisp` builds its per-stream data queue this way:
# `take` parks when the buffer is empty, and `put` grants one permit.
# A taker killed by a deadline must not cost the next taker its permit.

(println "one-permit channel survives a killed taker...")

(defn make-channel []
  "The unbounded cooperative FIFO from lib/http2/stream.lisp: `put`
   never blocks, `take` parks while empty, and each put wakes one taker."
  (let [key (gensym)
        bx (box 0)
        buf @[]
        @waiting false]
    {:put (fn [val]
            (push buf val)
            (when waiting
              (rebox bx (inc (unbox bx)))
              (ev/futex-wake key 1))
            nil)
     :take (fn []
             (while (= (length buf) 0)
               (assign waiting true)
               (let [gen (unbox bx)]
                 (when (= (length buf) 0) (ev/futex-wait key bx gen)))
               (assign waiting false))
             (let [val (get buf 0)]
               (remove buf 0)
               val))}))

(let [ch (make-channel)]
  # A taker that finds the channel empty and loses to its deadline.
  (assert (nil? (ev/timeout 0.05 (fn [] (ch:take))))
          "a take on an empty channel must lose to its deadline")
  # The next taker must still receive what the next put delivers.
  (let [taker (ev/spawn (fn [] (ch:take)))]
    (ev/sleep 0.05)
    (ch:put :payload)
    (assert (= :payload (join-by-deadline taker))
            "the next taker must receive the put")))

(println "park-abort: park queues hold only live waiters")
