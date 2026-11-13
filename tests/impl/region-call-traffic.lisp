(elle/epoch 15)
# audited: 2026-11-12
# A call hands its callee the frame's reference where the call is the argument's last use.
# docs/impl/region/handoff.md
# docs/impl/region/diagnostics.md
#
# Each shape is a loop that reads a string out of an array and does one thing
# with it. The file counts the increments and releases each item costs, on the
# interpreter and on compiled code, with `arena/increfs` and `arena/decrefs`.
#
# ── The counter-factual ──────────────────────────────────────────────
#
# A redundant pair is balanced: one count raised for the callee and dropped
# again by the frame leaks nothing and frees nothing early. So every leak and
# soundness gauge reads such a loop as correct, at twice the cost of the read
# alone. A predicate called through a parameter paid two counts and two
# releases per item, and three of each where it compared against a captured
# string. Only an exact count per item tells that from the one count the read
# itself takes, which the first shape pins.
#
# ── The trap ─────────────────────────────────────────────────────────
#
# A count per item is the difference between two runs of different lengths,
# so the setup each run pays cancels. A compile that lands inside a run adds
# the counts its code pin takes, so the sidecar compiles on the first call, on
# the VM thread, and each length keeps the smallest of three runs.

(def words (->array (map (fn [k] (string "w" k)) (range 256))))

# ── The callees ──────────────────────────────────────────────────────

(defn ignore [g]
  nil)
(def bar "|")
(defn is-bar [g]
  (identical? g bar))
(def is-w0
  (let [w (string "w" 0)]
    (fn [g] (identical? g w))))
(defn reset [@g]
  (assign g nil)
  nil)
(defn collect [& gs]
  nil)

# ── The walks ────────────────────────────────────────────────────────
#
# Each takes the callee as a parameter, so no walk can inline or graft it.

(defn walk-read [p? n]
  (var i 0)
  (while (< i n)
    (words i)
    (assign i (inc i)))
  i)

(defn walk [p? n]
  (var i 0)
  (while (< i n)
    (p? (words i))
    (assign i (inc i)))
  i)

(defn walk-keep [p? n]
  (var i 0)
  (while (< i n)
    (let [t (words i)]
      (p? t)
      (identical? t t))
    (assign i (inc i)))
  i)

# ── The measurement ──────────────────────────────────────────────────

(def short 64)
(def long 192)

(defn run-once [tier walker p? n]
  (def i0 (arena/increfs))
  (def d0 (arena/decrefs))
  (def o0 (arena/count))
  (compile/run-on tier walker p? n)
  [(- (arena/increfs) i0) (- (arena/decrefs) d0) (- (arena/count) o0)])

(defn least [tier walker p? n]
  (var best (run-once tier walker p? n))
  (repeat 2
    (let [r (run-once tier walker p? n)]
      (assign
        best
        [(min (best 0) (r 0)) (min (best 1) (r 1)) (min (best 2) (r 2))])))
  best)

(def span (- long short))

(defn per-span [tier walker p?]
  # One run first, so a compile `compile/run-on` makes lands in no reading.
  (run-once tier walker p? 1)
  (let [a (least tier walker p? short)
        b (least tier walker p? long)]
    [(- (b 0) (a 0)) (- (b 1) (a 1)) (- (b 2) (a 2))]))

(defn expect [tier label walker p? increfs decrefs]
  (let [[i d o] (per-span tier walker p?)]
    (assert (= i (* increfs span))
            (string tier " " label ": " increfs " counts an item, got " i
                    " over " span " items"))
    (assert (= d (* decrefs span))
            (string tier " " label ": " decrefs " releases an item, got " d
                    " over " span " items"))
    (assert (= o 0)
            (string tier " " label ": the heap grows " o " over " span " items"))))

(defn same [tier label p?]
  (let [[i d o] (per-span tier walk p?)
        [ki kd _] (per-span tier walk-keep p?)]
    (assert (and (= i ki) (= d kd))
            (string tier " " label ": " i "/" d " over " span
                    " items, where the frame keeps the value " ki "/" kd))
    (assert (= o 0)
            (string tier " " label ": the heap grows " o " over " span " items"))))

# ── The tiers ────────────────────────────────────────────────────────
#
# A build with no JIT compiled in rejects the `:jit` tier, and runs the
# interpreter's rows alone.

(def tiers
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      [:bytecode]
      [:bytecode :jit])))

(each tier tiers
  # The control: the read takes one count, and the frame's release gives it back.
  (expect tier "the read alone" walk-read ignore 1 1)
  # The call is the read's last use, so the frame's reference moves.
  (expect tier "a parameter it ignores" walk ignore 1 1)
  # The tail call in `is-bar` mints `bar` for a native, which needs none.
  (expect tier "a global it compares" walk is-bar 1 1)
  (expect tier "a capture it compares" walk is-w0 1 1)
  # A native callee takes no reference, and the offer is declined.
  (expect tier "a native" walk length 1 1)
  # The frame reads `t` after the call: a mint for the callee, and its release.
  (expect tier "the value read after the call" walk-keep ignore 2 2)
  # A cell parameter and a rest position decline the move: each costs what the
  # same callee costs where the frame still reads the value.
  (same tier "a cell parameter" reset)
  (same tier "a rest position" collect))

(println "region-call-traffic: ok")
