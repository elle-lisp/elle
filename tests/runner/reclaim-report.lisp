(elle/epoch 18)
## audited: 2026-11-22
## Under --dump=stats a process reports every reclamation counter, summed over every heap it ran.
## tests/runner/overview.md
## docs/impl/region/diagnostics.md
##
## The counter-factual: a report that read the main heap alone would still
## print every line, and every identity would still close. Only a count that
## must arrive from another heap tells the two apart, so the worker and
## `elle test` cases compare a run whose other heap adopts against one whose
## other heap does not.

(include-file "lib/harness.lisp")

# Every line of the report, in the order diagnostics.md's table gives.
(def names
  ["region-frees" "page-frees" "object-frees" "one-page-frees" "empty-frees"
   "one-object-frees" "few-object-frees" "many-object-frees" "adopts"
   "adopts-into-empty" "owned-frees" "owned-free-pages" "owned-free-objects"
   "owned-one-page-frees" "rescues" "rescue-survivors" "extracts" "reparents"
   "owned"])

# A function whose push adopts the pushed array into the container, and whose
# return frees both: one adoption per call, ended by an owned free.
#
# The trap: the array holds the call's argument. The unit builds an array of
# constants once, before any call, so 50 calls would push one array and never
# adopt it.
(def keep-one
  (string "(defn keep-one [x]\n  (let [c (@array)]\n"
          "    (push c (array x 2))\n    nil))\n"
          "(defn keep-many [n]\n  (var i 0)\n  (while (< i n)\n"
          "    (keep-one i)\n    (assign i (+ i 1))))\n"))

(defn report [stderr]
  "The report's lines in STDERR, as [name value] in printed order."
  (map (fn [l]
         (let [[k v] (string/split (slice l (length "[stats] reclaim ")) "=")]
           [k (parse-int (string/trim v))]))
       (filter (fn [l] (string/starts-with? l "[stats] reclaim "))
               (lines stderr))))

(defn value [got name]
  (let [hit (filter (fn [kv] (= (get kv 0) name)) got)]
    (assert (not (empty? hit)) (string "no " name " line in " got))
    (get (first hit) 1)))

(with-temp-dir dir
  (defn run [tag args src]
    "Run `elle ARGS` over SRC, written to a file, and answer its stderr."
    (let [r (elle-run [;args (write-fixture dir (string tag ".lisp") src)])]
      (assert (= r:exit 0) (string "elle " args " failed: " r:err))
      r:err))

  (eprintln "scenario: stats reports every counter in order, and the buckets close")
  (let [got (report (run "report" ["--dump=stats"]
                         (string keep-one "(keep-many 50)\n")))
        frees (value got "region-frees")]
    (assert (= (string/join (map (fn [kv] (get kv 0)) got) ",")
               (string/join names ","))
            (string "the report prints every counter once, in the table's "
                    "order: " got))
    (assert (> frees 0) "the run freed regions, so the sums are not vacuous")
    (assert (= frees
               (+ (value got "empty-frees") (value got "one-object-frees")
                  (value got "few-object-frees") (value got "many-object-frees")))
            "summed over the process, the object buckets partition region-frees")
    (assert (<= (value got "one-page-frees") frees)
            "a one-page free is a region free")
    (assert (>= (value got "adopts") 50)
            (string "each of the 50 calls adopts once: " got))
    (assert (>= (value got "owned") 0)
            (string "owned counts adoptions not yet ended: " got)))

  (eprintln "scenario: without stats there is no report")
  (let [stderr (run "quiet" [] (string keep-one "(keep-many 5)\n"))]
    (assert (empty? (report stderr))
            (string "a run without --dump=stats prints no reclaim line: " stderr)))

  # The two programs differ only in what the worker does, so the difference
  # in adoptions is the worker heap's alone.
  (eprintln "scenario: the report counts the heap of a worker thread")
  (let [idle (value (report (run "idle" ["--dump=stats"]
                                 (string keep-one
                                   "(os/join (os/spawn (fn [] nil)))\n")))
                    "adopts")
        busy (value (report (run "busy" ["--dump=stats"]
                                 (string keep-one
                                   "(os/join (os/spawn (fn [] (keep-many 200))))\n")))
                    "adopts")]
    (assert (>= (- busy idle) 200)
            (string "the worker's 200 adoptions must reach the report: idle "
                    idle ", busy " busy)))

  # The runner's own heap adopts nothing for either file (docs/test-gauges.md);
  # each tier run of the adopting form adopts once, on a worker's heap. The
  # runner ends through os/exit, so this also covers the report on that path.
  (eprintln "scenario: elle test reports the heaps its test code ran on")
  (defn adopts-under-test [name src]
    (let [r (elle-test (path/join dir (string name ".db"))
                       ["--dump=stats"
                        (write-fixture dir (string name ".lisp") src)])]
      (assert (= r:exit 0)
              (string "a passing form must gate green; stderr: " r:err))
      (value (report r:err) "adopts")))
  (let [plain (adopts-under-test "plain" "(assert true \"ok\")\n")
        adopting (adopts-under-test "adopting"
                                    "(let [c (@array)]\n  (push c (array (length c) 2))\n  (length c))\n")]
    (assert (> adopting plain)
            (string "the adopting form's worker adoptions must reach the "
                    "report: plain " plain ", adopting " adopting)))

  (eprintln "all reclaim-report scenarios passed"))
