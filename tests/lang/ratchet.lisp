(elle/epoch 14)
# audited: 2026-10-04
# The ratchet's judge, row reader and line reader, as the pure functions of the
# ledger module the runner judges with (docs/ratchet.md).
#
# The counter-factual: `match-rate?` accepted a reading within half a unit of
# its pin, so a shape pinned at 0 passed at 0.3 objects per operation. The
# judge below compares the reading's own interval, and nothing else.
(def l ((import "std/ratchet/ledger")))

# ── rows ──────────────────────────────────────────────────────────────
(def pin-row (l:parse-row (first (read-all "[\"io-drop\" :objects 0]"))))
(assert (= pin-row:subject "io-drop") "a row names its subject")
(assert (= pin-row:axis :objects) "and its axis, as a keyword")
(assert (= pin-row:kind :pin) "a bare number is a pin")
(assert (= pin-row:bound 0) "at that number")
(assert (= pin-row:class :control) "a row with no class is a control")
(assert (= pin-row:better :lower) "and lower is the better side")
(assert (= pin-row:slack 0) "with no slack")
(assert (nil? pin-row:build) "and no build: the reference build's")

(def floor-row
  (l:parse-row (first (read-all "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth]"))))
(assert (= floor-row:kind :floor) ":floor N is a floor")
(assert (= floor-row:bound 0.5) "at N")
(assert (= floor-row:class :growth) "and the class is read")

(def ceiling-row
  (l:parse-row (first (read-all "[\"append ratio\" :time :ceiling 8 :slack 0.5 :better :higher :root :f1a :note \"why\"]"))))
(assert (= ceiling-row:kind :ceiling) ":ceiling N is a ceiling")
(assert (= ceiling-row:bound 8) "at N")
(assert (= ceiling-row:slack 0.5) "every option is read")
(assert (= ceiling-row:better :higher) "including the better side")
(assert (= ceiling-row:root :f1a) "and the root")
(assert (= ceiling-row:note "why") "and the note")

(assert (nil? (l:parse-row (first (read-all "(producer \"x\")"))))
        "the header is not a row")
(assert (nil? (l:parse-row (first (read-all "(elle/epoch 13)"))))
        "and neither is the epoch declaration")

(assert (= (l:row-key "io-drop" :objects) (l:row-key "io-drop" :objects))
        "a key is a function of the subject and the axis")
(assert (not= (l:row-key "io-drop" :objects) (l:row-key "io-drop" :regions))
        "and two axes of one subject are two keys")

# ── a row belongs to one build ────────────────────────────────────────
# The counter-factual: a row with no :build was every build's row wherever a
# build had none of its own, so a build that never read a pin was judged
# against the reference build's footprint.
(def foreign "interp-pool-plan9-mips")
(with-temp-dir dir (def path (path/join dir "p.lisp"))
               (spit path
                     (string "(elle/epoch 13)\n(producer \"p\")\n"
                             "[\"plain\" :count 1]\n"
                             "[\"named\" :count 2 :build \"" l:reference-build
                             "\"]\n" "[\"foreign\" :count 3 :build \"" foreign
                             "\"]\n"))
               (def home (get (l:load-file path l:reference-build) :rows))
               (assert (= (get (get home (l:row-key "plain" :count)) :bound) 1)
                       "a row with no :build belongs to the reference build")
               (assert (= (get (get home (l:row-key "named" :count)) :bound) 2)
                       "and so does a row naming the reference key")
               (assert (nil? (get home (l:row-key "foreign" :count)))
                       "a row of another build is no row on the reference build")
               (def away (get (l:load-file path foreign) :rows))
               (assert (= (length (keys away)) 1)
                       "another build loads its own rows alone")
               (assert (= (get (get away (l:row-key "foreign" :count)) :bound) 3)
                       "which are the rows naming it")
               (def none (get (l:load-file path "mlir-pool-plan9-mips") :rows))
               (assert (empty? (keys none))
                       "a build with no rows of its own loads none, not the reference build's"))

# ── the judge ─────────────────────────────────────────────────────────
(def pin {:kind :pin :bound 1.0 :better :lower :slack 0.0})
(assert (= (l:judge {:value 1.0 :half 0.0} pin) :ok) "at the pin")
(assert (= (l:judge {:value 1.05 :half 0.1} pin) :ok) "overlapping the pin")
(assert (= (l:judge {:value 1.3 :half 0.1} pin) :regression) "confidently above")
(assert (= (l:judge {:value 0.0 :half 0.1} pin) :stale) "confidently below")
(assert (= (l:judge {:value 1.3 :half 0.5} pin) :ok)
        "an interval that straddles the pin cannot fail it")
(assert (= (l:judge {:value 0.3 :half 0.0} {:kind :pin :bound 0}) :regression)
        "0.3/op against a pin of 0 is a regression, not integer noise")
(assert (= (l:judge {:value 5} nil) :unledgered) "no row")
(assert (= (l:judge {:value 5} {:kind :pin :bound 5}) :ok)
        "half, slack and better all default")

(def rising {:kind :pin :bound 10 :better :higher :slack 0})
(assert (= (l:judge {:value 9 :half 0} rising) :regression)
        "under :better :higher a fall is the regression")
(assert (= (l:judge {:value 12 :half 0} rising) :stale) "and a rise is stale")

(def slack {:kind :pin :bound 100 :better :lower :slack 10})
(assert (= (l:judge {:value 108 :half 0} slack) :ok)
        "slack widens the worse side")
(assert (= (l:judge {:value 92 :half 0} slack) :ok) "and the better side")
(assert (= (l:judge {:value 111 :half 0} slack) :regression) "and no further")

(def floor {:kind :floor :bound 0.5 :better :lower :slack 0.0})
(assert (= (l:judge {:value 0.9 :half 0.2} floor) :ok) "above a floor")
(assert (= (l:judge {:value 0.4 :half 0.2} floor) :ok) "reaching a floor")
(assert (= (l:judge {:value 0.1 :half 0.2} floor) :regression) "below a floor")
(assert (= (l:judge {:value 50 :half 0} floor) :ok) "a floor is never stale")

(def ceiling {:kind :ceiling :bound 8 :better :lower :slack 0.0})
(assert (= (l:judge {:value 9.0 :half 0.5} ceiling) :regression)
        "above a ceiling")
(assert (= (l:judge {:value 8.0 :half 0.5} ceiling) :ok) "reaching a ceiling")
(assert (= (l:judge {:value 0 :half 0} ceiling) :ok) "a ceiling is never stale")

(assert (= (l:judge {:value 0 :half 0 :void "block-dependent"} pin) :void)
        "a reading the instrument refused stays void whatever the row says")

# ── judge-all: a dead gauge voids its axis ────────────────────────────
(def rows
  @{(l:row-key "w gauge (live-growth)" :w) {:kind :floor
    :bound 0.5
    :class :growth}
    (l:row-key "thing" :w) {:kind :pin :bound 0}
    (l:row-key "other" :count) {:kind :pin :bound 1}})
(def judged
  (l:judge-all [{:subject "w gauge (live-growth)" :axis :w :value 0.0 :half 0.0}
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
# The line is the reading as the instrument took it. A bound, a kind and a
# verdict are the runner's, so a reading that carries them prints none.
(def line
  (l:render-reading {:subject "a b"
                     :axis :objects
                     :value 0.5
                     :half 0.1
                     :unit "objects/op"
                     :class :growth
                     :floor 0.5
                     :void "block-dependent"
                     :alt-value 0.7
                     :bound 0
                     :kind :pin
                     :verdict :regression}))
(assert (string/starts-with? line "measure {") "a reading renders as one line")
(assert (not (string/contains? line "verdict")) "with no verdict")
(assert (not (string/contains? line "bound")) "and no bound")
(assert (not (string/contains? line "\"kind\"")) "and no kind")
(def back (l:readings-in (string "noise before\n" line "\nand after\n")))
(assert (= (length back) 1) "the line reads back, and the noise does not")
(def got (get back 0))
(assert (= got:subject "a b") "the subject")
(assert (= got:axis :objects) "the axis, as a keyword again")
(assert (= got:value 0.5) "the value")
(assert (= got:half 0.1) "the half-width")
(assert (= got:unit "objects/op") "the unit")
(assert (= got:class :growth) "the class, as a keyword")
(assert (= got:floor 0.5) "the floor a growth reading names")
(assert (= got:void "block-dependent") "the reason the instrument refused it")
(assert (= got:alt-value 0.7) "and the rate at the other block size")
(def bare
  (get (l:readings-in "measure {\"subject\":\"x\",\"axis\":\"count\",\"value\":3}")
       0))
(assert (= bare:half 0) "half defaults to 0 when the line omits it")
(assert (nil? bare:verdict) "and a line carries no verdict")

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
          "[\"odd \\\"q\\\" ]\" :count 1]\n" "[\"answer\" :count 41 :build \""
          foreign "\"]\n"))

(def moved (rp:move ledger-text "answer" :count "42" nil))
(assert (string/contains? moved "[\"answer\" :count 42]\n")
        "the reference build's pin token is replaced")
(assert (string/contains? moved
                          (string "[\"answer\" :count 41 :build \"" foreign
                                  "\"]"))
        "and another build's row of the same subject is left alone")
(assert (string/contains? moved "# a comment that must survive\n")
        "the comment above the row survives")
(assert (string/contains? moved
                          "[\"long subject\"\n :count\n 7 :note \"wrapped by fmt\"]")
        "and every other row is byte for byte what it was")

(def moved-away (rp:move ledger-text "answer" :count "40" foreign))
(assert (string/contains? moved-away
                          (string "[\"answer\" :count 40 :build \"" foreign
                                  "\"]"))
        "a build's own row moves when the build is named")
(assert (string/contains? moved-away "[\"answer\" :count 43]\n")
        "and the reference build's row stays")

(def wrapped (rp:move ledger-text "long subject" :count "9" nil))
(assert (string/contains? wrapped
                          "[\"long subject\"\n :count\n 9 :note \"wrapped by fmt\"]")
        "a wrapped row keeps its wrapping and moves its bound")

(def odd (rp:move ledger-text "odd \"q\" ]" :count "2" nil))
(assert (string/contains? odd "[\"odd \\\"q\\\" ]\" :count 2]")
        "a subject holding a quote and a bracket is still found by its string")

(assert (nil? (rp:move ledger-text "nobody" :count "1" nil)) "no row, no move")

(def adopted (rp:adopt ledger-text "[\"new\" :count 5]"))
(assert (string/ends-with? adopted "[\"new\" :count 5]\n")
        "an adopted row is appended after the last row, on a line of its own")

(assert (= (rp:row-for {:subject "x" :axis :count :value 42 :half 0} nil)
           "[\"x\" :count 42]")
        "an unledgered reading becomes a pin at its value")
(assert (= (rp:row-for {:subject "x" :axis :count :value 42 :half 0} foreign)
           (string "[\"x\" :count 42 :build \"" foreign "\"]"))
        "and names the build it was read on, away from the reference build")
(assert (= (rp:row-for {:subject "objects gauge (live-growth)"
                        :axis :objects
                        :value 1.0
                        :half 0.0
                        :class :growth
                        :floor 0.5} nil)
           "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth]")
        "and a growth reading becomes a growth floor at the floor it named")
(assert (= (rp:row-for {:subject "say \"hi\"" :axis :ms :value 1.2345 :half 0.2}
                       nil) "[\"say \\\"hi\\\"\" :ms 1.23]")
        "the subject is written as a string literal, the rate to three figures")

(println "ratchet: ok")
