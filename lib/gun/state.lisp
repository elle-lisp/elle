(elle/epoch 15)
# audited: 2026-10-21
## The hybrid logical clock that stamps each write, and the only comparison of two states.
## lib/gun.md
## lib/gun/overview.md
##
## Loaded via:
##   (def state ((import "std/gun/state")))
##
## Exports: {:make-clock :pack :unpack :before? :format :parse}
##
## A state is one integer: milliseconds shifted left 16 bits, plus a counter.
## Code outside this file compares states only through before?, so the
## representation can change here alone.

(fn []

  (def counter-bits 16)
  (def counter-mask 0xFFFF)

  (defn pack [ms counter]
    "The state of millisecond ms with counter. A counter past 0xFFFF carries
     into the millisecond, so a packed state never loses order."
    (+ (bit/shl ms counter-bits) counter))

  (defn unpack [s]
    "The [ms counter] of state s."
    [(bit/shr s counter-bits) (bit/and s counter-mask)])

  (defn before? [a b]
    "True when state a is before state b. nil is no state, before every state."
    (cond
      (nil? b) false
      (nil? a) true
      (< a b)))

  (defn format [s]
    "State s as decimal text."
    (string s))

  (defn parse [text]
    "The state that format wrote as text. Raises :bad-state on anything else."
    (let [[ok? n] (protect (parse-int text))]
      (unless (and ok? (= (string n) text) (>= n 0))
        (error {:error :gun-error
                :reason :bad-state
                :text text
                :message (string "not a state: " text)}))
      n))

  (defn wall-ms []
    "The wall clock in whole milliseconds."
    (integer (* 1000 (clock/realtime))))

  (defn make-clock [&named @now-ms]
    "A hybrid logical clock. :now-ms answers the wall clock in milliseconds;
     the default reads clock/realtime.

     :tick answers a state after every state it answered before and after
     every state :observe saw. :now answers the latest state a write may
     carry: the end of the current millisecond, or the latest state observed
     when that is later. :due-in answers the seconds until a state is no
     longer after :now, by the wall clock :now-ms reads, and 0 when it is due."
    (default now-ms wall-ms)
    (def @last nil)
    (defn now []
      (let [end (- (pack (+ (now-ms) 1) 0) 1)]
        (if (before? end last) last end)))
    {:tick (fn []
             (let [wall (pack (now-ms) 0)
                   next (if (before? last wall) wall (+ last 1))]
               (assign last next)
               next))
     :observe (fn [s]
                (when (before? last s) (assign last s))
                nil)
     :now now
     :due-in (fn [s]
               (if (before? (now) s)
                 (/ (- (first (unpack s)) (now-ms)) 1000.0)
                 0))})

  {:make-clock make-clock
   :pack pack
   :unpack unpack
   :before? before?
   :format format
   :parse parse})
