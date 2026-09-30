(elle/epoch 13)
# audited: 2026-09-30
# The ratchet's judge, row reader and line reader, as the pure functions the
# library and the runner share (docs/ratchet.md).
#
# The counter-factual: `match-rate?` accepted a reading within half a unit of
# its pin, so a shape pinned at 0 passed at 0.3 objects per operation. The
# judge below compares the reading's own interval, and nothing else.
(def r ((import "std/ratchet")))

# ── rows ──────────────────────────────────────────────────────────────
(def pin-row (r:parse-row (first (read-all "[\"io-drop\" :objects 0]"))))
(assert (= pin-row:subject "io-drop") "a row names its subject")
(assert (= pin-row:axis :objects) "and its axis, as a keyword")
(assert (= pin-row:kind :pin) "a bare number is a pin")
(assert (= pin-row:bound 0) "at that number")
(assert (= pin-row:class :control) "a row with no class is a control")
(assert (= pin-row:better :lower) "and lower is the better side")
(assert (= pin-row:slack 0) "with no slack")

(def floor-row
  (r:parse-row (first (read-all "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth]"))))
(assert (= floor-row:kind :floor) ":floor N is a floor")
(assert (= floor-row:bound 0.5) "at N")
(assert (= floor-row:class :growth) "and the class is read")

(def ceiling-row
  (r:parse-row (first (read-all "[\"append ratio\" :time :ceiling 8 :slack 0.5 :better :higher :root :f1a :note \"why\"]"))))
(assert (= ceiling-row:kind :ceiling) ":ceiling N is a ceiling")
(assert (= ceiling-row:bound 8) "at N")
(assert (= ceiling-row:slack 0.5) "every option is read")
(assert (= ceiling-row:better :higher) "including the better side")
(assert (= ceiling-row:root :f1a) "and the root")
(assert (= ceiling-row:note "why") "and the note")

(assert (nil? (r:parse-row (first (read-all "(producer \"x\")"))))
        "the header is not a row")
(assert (nil? (r:parse-row (first (read-all "(elle/epoch 13)"))))
        "and neither is the epoch declaration")

(assert (= (r:row-key "io-drop" :objects) (r:row-key "io-drop" :objects))
        "a key is a function of the subject and the axis")
(assert (not= (r:row-key "io-drop" :objects) (r:row-key "io-drop" :regions))
        "and two axes of one subject are two keys")

# ── the judge ─────────────────────────────────────────────────────────
(def pin {:kind :pin :bound 1.0 :better :lower :slack 0.0})
(assert (= (r:judge {:value 1.0 :half 0.0} pin) :ok) "at the pin")
(assert (= (r:judge {:value 1.05 :half 0.1} pin) :ok) "overlapping the pin")
(assert (= (r:judge {:value 1.3 :half 0.1} pin) :regression) "confidently above")
(assert (= (r:judge {:value 0.0 :half 0.1} pin) :stale) "confidently below")
(assert (= (r:judge {:value 1.3 :half 0.5} pin) :ok)
        "an interval that straddles the pin cannot fail it")
(assert (= (r:judge {:value 0.3 :half 0.0} {:kind :pin :bound 0}) :regression)
        "0.3/op against a pin of 0 is a regression, not integer noise")
(assert (= (r:judge {:value 5} nil) :unledgered) "no row")
(assert (= (r:judge {:value 5} {:kind :pin :bound 5}) :ok)
        "half, slack and better all default")

(def rising {:kind :pin :bound 10 :better :higher :slack 0})
(assert (= (r:judge {:value 9 :half 0} rising) :regression)
        "under :better :higher a fall is the regression")
(assert (= (r:judge {:value 12 :half 0} rising) :stale) "and a rise is stale")

(def slack {:kind :pin :bound 100 :better :lower :slack 10})
(assert (= (r:judge {:value 108 :half 0} slack) :ok)
        "slack widens the worse side")
(assert (= (r:judge {:value 92 :half 0} slack) :ok) "and the better side")
(assert (= (r:judge {:value 111 :half 0} slack) :regression) "and no further")

(def floor {:kind :floor :bound 0.5 :better :lower :slack 0.0})
(assert (= (r:judge {:value 0.9 :half 0.2} floor) :ok) "above a floor")
(assert (= (r:judge {:value 0.4 :half 0.2} floor) :ok) "reaching a floor")
(assert (= (r:judge {:value 0.1 :half 0.2} floor) :regression) "below a floor")
(assert (= (r:judge {:value 50 :half 0} floor) :ok) "a floor is never stale")

(def ceiling {:kind :ceiling :bound 8 :better :lower :slack 0.0})
(assert (= (r:judge {:value 9.0 :half 0.5} ceiling) :regression)
        "above a ceiling")
(assert (= (r:judge {:value 8.0 :half 0.5} ceiling) :ok) "reaching a ceiling")
(assert (= (r:judge {:value 0 :half 0} ceiling) :ok) "a ceiling is never stale")

(assert (= (r:judge {:value 0 :half 0 :void "block-dependent"} pin) :void)
        "a reading the instrument refused stays void whatever the row says")

# ── judge-all: a dead gauge voids its axis ────────────────────────────
(def rows
  @{(r:row-key "w gauge (live-growth)" :w) {:kind :floor
    :bound 0.5
    :class :growth}
    (r:row-key "thing" :w) {:kind :pin :bound 0}
    (r:row-key "other" :count) {:kind :pin :bound 1}})
(def judged
  (r:judge-all [{:subject "w gauge (live-growth)" :axis :w :value 0.0 :half 0.0}
                {:subject "thing" :axis :w :value 0.0 :half 0.0}
                {:subject "other" :axis :count :value 1 :half 0}] rows))
(assert (= (get (get judged 0) :verdict) :void) "the flat growth row is void")
(assert (= (get (get judged 1) :verdict) :void)
        "and so is a reading of 0 on that axis, which would have been ok")
(assert (string? (get (get judged 1) :why)) "with the reason attached")
(assert (= (get (get judged 2) :verdict) :ok) "another axis is untouched")
(assert (= (get (get judged 2) :bound) 1) "and every reading learns its bound")
(assert (= (get (get judged 2) :kind) :pin) "and its row's kind")

# ── the line ──────────────────────────────────────────────────────────
(def line
  (r:render-reading {:subject "a b"
                     :axis :objects
                     :value 0.5
                     :half 0.1
                     :unit "objects/op"
                     :bound 0
                     :kind :pin
                     :verdict :regression}))
(assert (string/starts-with? line "measure {") "a reading renders as one line")
(def back (r:readings-in (string "noise before\n" line "\nand after\n")))
(assert (= (length back) 1) "the line reads back, and the noise does not")
(def got (get back 0))
(assert (= got:subject "a b") "the subject")
(assert (= got:axis :objects) "the axis, as a keyword again")
(assert (= got:value 0.5) "the value")
(assert (= got:half 0.1) "the half-width")
(assert (= got:unit "objects/op") "the unit")
(assert (= got:verdict :regression) "the verdict, as a keyword")
(def bare
  (get (r:readings-in "measure {\"subject\":\"x\",\"axis\":\"count\",\"value\":3}")
       0))
(assert (= bare:half 0) "half defaults to 0 when the line omits it")
(assert (nil? bare:verdict) "and a line nobody judged carries no verdict")

# ── a drive: the rate over a run-block of the caller's own ────────────
# The counter-factual: a tail recursion performs its b ops in one call, a
# fiber is drained after b yields, and a strand needs its op run as a
# discarded statement. No per-op thunk expresses those, so each dashboard
# kept a private estimator for the shape. This file has no ledger, so every
# reading here is printed unjudged and read back off the struct.
(def @kept @[])
(defn keep-n [b]
  (when (%not (%int? b)) (error :b))
  (def @i 0)
  (while (%lt i b)
    (push kept {:x i})
    (assign i (%add i 1))))
(defn drop-n [b]
  (when (%not (%int? b)) (error :b))
  (def @i 0)
  (while (%lt i b)
    {:x i}
    (assign i (%add i 1))))
(def driven (get (r:drive "kept loop" keep-n :block 50 :min 4 :max 10) 0))
(assert (= driven:subject "kept loop") "a drive's reading names its subject")
(assert (= driven:axis :objects) "on the default gauge")
(assert (< 0.5 driven:value) "and reads the growth the run-block makes, per op")
(def dropped (get (r:drive "dropped loop" drop-n :block 50 :min 4 :max 10) 0))
(assert (= dropped:value 0.0) "a loop that drops its structs reads 0")
(def two
  (r:drive "two gauges" drop-n :on [r:objects r:regions] :block 50 :min 4
           :max 10))
(assert (= (length two) 2) "one reading per gauge in :on")
(assert (= (get (get two 1) :axis) :regions) "in the order given")
(def stmt
  (get (r:drive "dropped statement" (r:stmt-run (fn [] {:y 1})) :block 50 :min 4
                :max 10) 0))
(assert (= stmt:subject "dropped statement")
        "stmt-run makes a run-block of a thunk")
(assert (= stmt:value 0.0) "and a struct dropped as a statement costs nothing")

# ── the re-pin: a bound's token moves, and nothing else in the file ──
# The counter-factual: a re-pin that rewrote the row's line would drop the
# comment above it and undo the wrapping `elle fmt` gave a long row, so the
# rewrite scans for the row's brackets and replaces one token.
(def rp ((import "std/ratchet/repin")))

(assert (= (rp:write-number 42.0 0) "42") "a count is the integer it is")
(assert (= (rp:write-number 42 0) "42") "whether it arrived as one or not")
(assert (= (rp:write-number 1.31234 0.1) "1.31")
        "a rate is three significant figures")
(assert (= (rp:write-number 0.047512 0.001) "0.0475") "below one as well")
(assert (= (rp:write-number 1234.5 1) "1230") "and above a thousand")
(assert (= (rp:write-number 0.0 0.03) "0") "a rate of nothing is 0")
(assert (= (rp:write-number 2.0 0.03) "2") "and an integral rate drops its .0")

(def ledger-text
  (string "(elle/epoch 13)\n" "# a comment that must survive\n"
          "(producer \"p\")\n" "[\"answer\" :count 43]\n" "[\"long subject\"\n"
          " :count\n" " 7 :note \"wrapped by fmt\"]\n"
          "[\"odd \\\"q\\\" ]\" :count 1]\n"))

(def moved (rp:move ledger-text "answer" :count "42"))
(assert (string/contains? moved "[\"answer\" :count 42]")
        "the pin's token is replaced")
(assert (string/contains? moved "# a comment that must survive\n")
        "the comment above the row survives")
(assert (string/contains? moved
                          "[\"long subject\"\n :count\n 7 :note \"wrapped by fmt\"]")
        "and every other row is byte for byte what it was")

(def wrapped (rp:move ledger-text "long subject" :count "9"))
(assert (string/contains? wrapped
                          "[\"long subject\"\n :count\n 9 :note \"wrapped by fmt\"]")
        "a wrapped row keeps its wrapping and moves its bound")

(def odd (rp:move ledger-text "odd \"q\" ]" :count "2"))
(assert (string/contains? odd "[\"odd \\\"q\\\" ]\" :count 2]")
        "a subject holding a quote and a bracket is still found by its string")

(assert (nil? (rp:move ledger-text "nobody" :count "1")) "no row, no move")

(def adopted (rp:adopt ledger-text "[\"new\" :count 5]"))
(assert (string/ends-with? adopted
                           "[\"odd \\\"q\\\" ]\" :count 1]\n[\"new\" :count 5]\n")
        "an adopted row is appended after the last row, on a line of its own")

(assert (= (rp:row-for {:subject "x" :axis :count :value 42 :half 0})
           "[\"x\" :count 42]")
        "an unledgered reading becomes a pin at its value")
(assert (= (rp:row-for {:subject "objects gauge (live-growth)"
                        :axis :objects
                        :value 1.0
                        :half 0.0
                        :class :growth
                        :floor 0.5})
           "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth]")
        "and a growth reading becomes a growth floor at the floor it named")
(assert (= (rp:row-for {:subject "say \"hi\"" :axis :ms :value 1.2345 :half 0.2})
           "[\"say \\\"hi\\\"\" :ms 1.23]")
        "the subject is written as a string literal, the rate to three figures")

(println "ratchet: ok")
