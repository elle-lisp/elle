(elle/epoch 18)
## audited: 2026-11-22
## A producer run directly prints one line per reading, carrying the reading alone, whatever ledger sits beside it.
## tests/cli/overview.md
## docs/ratchet.md
##
## The counter-factual: the instrument found a ledger beside the program and
## judged each reading as it landed, so a direct run printed a verdict the
## runner then judged again, from a second code path. Each scenario runs a
## scratch producer directly, beside a ledger that names it.

(include-file "../runner/lib/harness.lisp")

(def import-ratchet "(def r ((import \"std/ratchet\")))\n")

(defn producer-run [dir name rows source]
  "Write SOURCE as producer.lisp in a directory NAME under DIR, and ROWS as
   its ledger under tests/ledger, the layout the runner reads. Run the
   producer from there, and answer what it left."
  (let [home (path/join dir name)
        ledger (path/join home "tests" "ledger")]
    (file/mkdir-all ledger)
    (write-fixture home "producer.lisp" source)
    (write-fixture ledger "producer.lisp"
                   (string "(elle/epoch 13)\n(producer \"producer.lisp\")\n"
                           rows "\n"))
    (elle-run ["producer.lisp"] :cwd home)))

(defn measured [r]
  "The `measure` lines run R printed, in order."
  (filter (fn [l] (string/starts-with? l "measure ")) (lines r:out)))

(defn unjudged [line]
  "Require LINE to carry none of the fields the runner owns."
  (each field ["\"bound\"" "\"kind\"" "\"verdict\""]
    (assert (not (string/contains? line field))
            (string "the instrument judges nothing, so no line carries " field
                    ": " line))))

(defn value-of [r subject]
  "The value the `measure` line of run R for SUBJECT carries, as text."
  (let [key (string "\"subject\":\"" subject "\"")
        line (find (fn [l] (string/contains? l key)) (measured r))]
    (assert line (string "no reading for " subject ": " r:out r:err))
    (let [rest (slice line
                      (+ (string/find line "\"value\":") (length "\"value\":")))]
      (first (string/split (first (string/split rest "}")) ",")))))

(with-temp-dir dir
  # The row would judge the reading a regression. A producer that judged
  # itself would fail here and print the verdict.
  (eprintln "scenario: the line carries the reading alone")
  (let [r (producer-run dir "line" "[\"answer\" :count 41]"
                        (string import-ratchet "(r:read \"answer\" :count 42)\n"))
        printed (measured r)]
    (assert (= r:exit 0)
            (string "a producer fails on nothing it measured: " r:err))
    (assert (= (length printed) 1) (string "one reading, one line: " r:out))
    (each want ["\"subject\":\"answer\"" "\"axis\":\"count\"" "\"value\":42"
                "\"half\":0" "\"unit\":\"count\""]
      (assert (string/contains? (first printed) want)
              (string "the line carries " want ": " (first printed))))
    (unjudged (first printed)))

  (eprintln "scenario: the live-growth reading is printed first, with its class and floor")
  (let [r (producer-run dir "proven"
                        (string "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth]\n"
                                "[\"dropped\" :objects 0]")
                        (string import-ratchet
                                "(r:delta \"dropped\" (fn [] {:x 1}) :on [r:objects] :n 50)\n"))
        printed (measured r)]
    (assert (= r:exit 0) (string "the producer runs: " r:err))
    (assert (= (length printed) 2)
            (string "the discriminator and the reading: " r:out))
    (each want ["\"subject\":\"objects gauge (live-growth)\""
                "\"class\":\"growth\"" "\"floor\":0.5"]
      (assert (string/contains? (first printed) want)
              (string "the gauge is proven live first, carrying " want ": "
                      (first printed))))
    (assert (string/contains? (get printed 1) "\"subject\":\"dropped\"")
            (string "then the producer's own reading: " (get printed 1)))
    (each line printed
      (unjudged line)))

  # A drive is a rate over a run-block of the caller's own, and reads the heap,
  # which only an implementation test may assert. The counter-factual: a tail
  # recursion does its b ops in one call, a fiber drains after b yields, and a
  # strand runs its op as a discarded statement. No per-op thunk expresses
  # those, so each dashboard kept a private estimator for the shape.
  (eprintln "scenario: a drive reads the rate of a run-block of the caller's own")
  (let [r (producer-run dir "drives" ""
                        (string import-ratchet "(def @kept @[])\n"
                                "(defn keep-n [b]\n"
                                "  (when (%not (%int? b)) (error :b))\n"
                                "  (def @i 0)\n" "  (while (%lt i b)\n"
                                "    (push kept {:x i})\n"
                                "    (assign i (%add i 1))))\n"
                                "(defn drop-n [b]\n"
                                "  (when (%not (%int? b)) (error :b))\n"
                                "  (def @i 0)\n" "  (while (%lt i b)\n"
                                "    {:x i}\n" "    (assign i (%add i 1))))\n"
                                "(def driven (get (r:drive \"kept loop\" keep-n :block 50 :min 4 :max 10) 0))\n"
                                "(assert (= driven:subject \"kept loop\") \"a drive's reading names its subject\")\n"
                                "(assert (= driven:axis :objects) \"on the default gauge\")\n"
                                "(assert (< 0.5 driven:value) \"and reads the growth the run-block makes, per op\")\n"
                                "(def dropped (get (r:drive \"dropped loop\" drop-n :block 50 :min 4 :max 10) 0))\n"
                                "(assert (= dropped:value 0.0) \"a loop that drops its structs reads 0\")\n"
                                "(def two (r:drive \"two gauges\" drop-n :on [r:objects r:regions] :block 50 :min 4 :max 10))\n"
                                "(assert (= (length two) 2) \"one reading per gauge in :on\")\n"
                                "(assert (= (get (get two 1) :axis) :regions) \"in the order given\")\n"
                                "(def stmt (get (r:drive \"dropped statement\" (r:stmt-run (fn [j] {:y j})) :block 50 :min 4 :max 10) 0))\n"
                                "(assert (= stmt:subject \"dropped statement\") \"stmt-run makes a run-block of a probe\")\n"
                                "(assert (= stmt:value 0.0) \"and a struct dropped as a statement costs nothing\")\n"
                                "(println \"drives: ok\")\n"))]
    (assert (and (= r:exit 0) (string/contains? r:out "drives: ok"))
            (string "every drive reads what its run-block does: " r:out r:err))
    (each line (measured r)
      (unjudged line)))

  # The trap: the page-claim gauge counts a page whether or not it is freed, so
  # anything the instrument allocates between its two readings is charged to
  # the probe. A store into an array the compiler could not type went through
  # the general `put` call, and every block read one page more than its ops
  # claimed: 1/400 per op at a block of 400.
  (eprintln "scenario: the window claims no page of its own")
  (let [r (producer-run dir "window" ""
                        (string import-ratchet
                                "(r:rate \"nothing\" (fn [j] nil) :on [r:pages] :block 400 :min 6)\n"
                                "(r:delta \"nothing, delta\" (fn [] nil) :on [r:pages] :n 400)\n"))]
    (assert (= r:exit 0) (string "the producer runs: " r:err))
    (assert (= (value-of r "nothing") "0.0")
            (string "a rate over a probe that does nothing claims no page: "
                    r:out))
    (assert (= (value-of r "nothing, delta") "0.0")
            (string "nor does a delta over a body that does nothing: " r:out))))

(eprintln "all ratchet-lines scenarios passed")
