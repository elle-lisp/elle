(elle/epoch 18)
# audited: 2026-11-20
# std/check: a case that skips, from `assume` or a filter, through any protect, and the limit on skips past which `run` gives up.
# docs/stdlib/check.md
#
# The counter-factual: a skip read as a failure fails every filtered run, and
# a skip read as a pass lets a filter that rejects everything pass. A filter
# that draws again on a rejection still keeps only the values it should, so
# the enumeration below names each case in order, and a retry would repeat one.

(def check ((import "std/check")))

(include-file "check/helpers.lisp")

(def odd (check:filter (check:int 0 9) odd?))
(def never (check:filter (check:int 0 9) (fn [_] false)))

# ── assume ────────────────────────────────────────────────────────────
(assert (= true (check:assume 0)) "assume returns true on a true value")
(defn skipped-by [ok]
  "What a fiber that calls (check:assume OK) stops on, as [signal error]: the
   signal of its first stop, and the error it raises when resumed."
  (let [f (fiber/new (fn [] (check:assume ok)) |:check/skip :error|)]
    (fiber/resume f)
    (let [signal (fiber/signaled? f :check/skip)
          err (fiber/resume f)]
      [signal (get err :error)])))

(assert (= [true :check/skip] (skipped-by false))
        "assume emits :check/skip on false, and raises :check/skip when resumed")
(assert (= [true :check/skip] (skipped-by nil)) "and on nil")

(let [f (fiber/new (fn [] (protect (check:assume false))) |:check/skip|)]
  (fiber/resume f)
  (assert (fiber/signaled? f :check/skip) "no protect catches the signal"))

# ── the case skips through a protect ──────────────────────────────────
# The counter-factual: a skip that a protect catches lets the property run on
# with the case it excluded, so each even case below would fail.
(let [r (check:run (check:int 0 9)
                   (fn [n]
                     (protect (check:assume (odd? n)))
                     (odd? n)) :depth 9 :trials 0)]
  (assert (= true (r :passed)) "a property's protect does not stop a skip")
  (assert (= [5 5] [(r :cases) (r :skipped)]) "each even case skips"))

(let [r (check:run (check:int 0 9)
                   (fn [_]
                     (protect (check:assume false))
                     (error {:error :after :message "after the skip"})) :depth 3
                   :trials 0)]
  (assert (= [true 0 4] [(r :passed) (r :cases) (r :skipped)])
          "a case skips whatever its property does after the signal"))

(let [g (fn [draw]
          (let [n (draw 10)]
            (protect (check:assume (odd? n)))
            n))]
  (assert (= [1 3 5 7 9] (check:enumerate g 9))
          "and so does a case whose generator catches its own skip"))

(let [log @[]
      r (check:run (check:int 0 9)
                   (fn [n]
                     (defer
                       (push log n)
                       (check:assume (odd? n))
                       true)) :depth 3 :trials 0)]
  (assert (= [2 2] [(r :cases) (r :skipped)]) "even cases skip")
  (assert (= [0 1 2 3] (freeze log))
          "and a defer in the property cleans up after each skip"))

# ── what a filter makes ───────────────────────────────────────────────
(assert (= [1 3 5 7 9] (check:enumerate odd 9))
        "the enumeration leaves out each case the filter rejects")
(assert (= [] (check:enumerate never 9)) "and every case of a filter of none")

(let [xs (check:sample odd :seed 3 :count 20)]
  (assert (= 20 (length xs)) "a sample draws until it has :count values")
  (assert (all? odd? xs) "each one a value the filter keeps"))
(assert (= :check-gave-up (get (raises (fn [] (check:sample never :count 4)))
                               :error))
        "a sample gives up when every case skips")

# ── run: skipped cases are counted apart ──────────────────────────────
(defn evens-before-odds [n seed]
  "How many even digits a SplitMix64 stream from SEED gives before its Nth
   odd one: the random cases of `odd` that skip before N pass."
  (var evens 0)
  (var odds 0)
  (each x in (check:sample (check:int 0 9) :seed seed :count 1000)
    (when (< odds n)
      (if (odd? x) (assign odds (+ odds 1)) (assign evens (+ evens 1)))))
  evens)

(let [r (check:run odd (fn [n] (odd? n)) :trials 10 :seed 6)]
  (assert (= true (r :passed)) "a filtered run passes on the values it keeps")
  (assert (= 12 (r :cases)) "1 and 3 from the enumeration, and 10 trials")
  (assert (= (+ 3 (evens-before-odds 10 6)) (r :skipped))
          "0, 2 and 4 skip in the enumeration, and each even trial skips"))

(defn ascending? [[a b]]
  "Whether A is less than B, in a property that assumes it."
  (check:assume (< a b))
  (< a b))

(assert (= true
           (get (check:run (check:tuple (check:int 0 9) (check:int 0 9))
                           ascending?) :passed))
        "a property that assumes skips the cases it excludes")

# ── run: a filter shrinks to the simplest value it keeps ──────────────
(def evens (check:filter (check:int 0 1000) even?))
(each seed in (range 5)
  (let [r (check:run evens (fn [n] (< n 10)) :depth 0 :seed seed)]
    (assert (= 10 (r :value))
            (string "seed " seed ": filtered evens shrink to 10"))
    (assert (= :property (r :failed-in))
            "a skip in the generator is not a generator failure")))

(each seed in (range 5)
  (let [r (check:run (check:int 0 1000)
                     (fn [n]
                       (check:assume (odd? n))
                       (< n 30)) :depth 0 :seed seed)]
    (assert (= 31 (r :value))
            (string "seed " seed ": the shrinker never keeps a case that skips"))))

# ── run: the limit on skips ───────────────────────────────────────────
(let [r (check:run never (fn [_] true) :depth 0 :trials 5)]
  (assert (= false (r :passed)) "a run where every case skips gives up")
  (assert (= 0 (r :cases)) "no case passed or failed")
  (assert (= 51 (r :skipped)) "the one enumerated case, then 10 * 5 trials")
  (assert (= :check-gave-up (get (r :error) :error)) ":error says it gave up")
  (assert (nil? (r :value)) "and there is no counterexample")
  (assert (nil? (r :failed-in)) "and no place it failed"))

(assert (= 4 (get (check:run never (fn [_] true) :depth 0 :skips 3) :skipped))
        ":skips sets the limit")

# A random 0 would skip, and check:int names 0 as an edge, so the draw names
# none.
(let [late (check:filter (fn [draw] (draw 1001)) (fn [n] (> n 4)))
      r (check:run late (fn [_] true) :trials 2 :skips 3 :seed 1)]
  (assert (= true (r :passed))
          "the enumeration's skipped cases do not count towards the limit")
  (assert (= 5 (r :skipped)) "0 to 4 skip in the enumeration"))

(let [err (raises (fn [] (check:assert never (fn [_] true) :trials 5)))]
  (assert (= :check-gave-up (err :error))
          "assert raises the error of a run that gave up"))
(let [err (raises (fn []
                    (check:assert odd (fn [_] true) :depth 0 :skips 1 :seed 6)))]
  (assert (= :check-gave-up (err :error)) "and passes :skips to run"))

(println "check-skip: all tests passed")
