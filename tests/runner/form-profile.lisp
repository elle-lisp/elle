(elle/epoch 18)
## audited: 2026-11-22
## A form row says what the compiler found in the form: the signals it may emit, the capabilities among them, and what it calls.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: the three columns existed and nothing wrote them, so
## every form read the same NULL. Selection by capability had no data, nothing
## could ask which forms are pure, and a cache could not tell a replayable form
## from one that opens a file.
##
## The trap: an empty column and a NULL are different claims. Empty says the
## compiler proved the form reaches no capability; NULL says the analysis did
## not answer. A test that accepted either would pass against a runner that
## analyzed nothing at all.

(include-file "lib/harness.lisp")

# Every capability the root fiber holds but `error`, `yield` and `halt`, sorted
# and space-separated as the `caps` column writes them. A bit the runtime gains
# joins the list here with no edit.
(def built-in-caps
  (let [@names @[]]
    (each k (fiber/caps)
      (unless (contains? |:error :yield :halt| k) (push names (string k))))
    (string/join (sort names) " ")))

(with-temp-dir dir
  (def db (path/join dir "s.db"))
  (def written (path/join dir "written.txt"))

  (def fixtures
    [["rich.lisp"
      # Writes a file and reads it back through a helper it defines: its
      # capabilities are fs and io, and write-it is its own.
      (string "(defn write-it [p]\n  (let [out (port/open p :write)]\n"
              "    (port/write out \"hi\")\n    (port/close out)))\n"
              "(write-it \"" written "\")\n" "(assert (= (slurp \"" written
              "\") \"hi\") \"the file round-trips\")\n")]
     ["pure.lisp" "(assert (= 1 1) \"arithmetic holds\")\n"]
     ["refused.lisp"
      # A file registers each signal it declares before its first form, and a
      # body registers one at its declaration, so the body's emit names an
      # unknown signal and the analysis refuses.
      (string "(defn raise-it [] (emit :form-profile-late 1))\n"
              "(signal :form-profile-late)\n"
              "(assert (fn? raise-it) \"the file runs\")\n")]
     ["ambient.lisp" "(assert (> (sys/pid) 0) \"the pid is positive\")\n"]
     ["unresolved.lisp"
      # A call through a table, which the compiler cannot resolve.
      (string "(def table @{:answer (fn [] 42)})\n"
              "(assert (= ((get table :answer)) 42) \"the table's function answers\")\n")]])

  (let [r (elle-test db
                     (map (fn [[name src]] (write-fixture dir name src))
                          fixtures))]
    (assert (= r:exit 0)
            (string "every fixture passes, so the run gates green; stderr: "
                    r:err)))

  (defn form [file cols]
    "The columns COLS of FILE's form row."
    (get (rows db
               (string "SELECT " cols " FROM form WHERE file LIKE '%/" file "'"))
         0))

  (defn holds? [file column word]
    "Whether the space-separated COLUMN of FILE's row names WORD. A column is a
     list of names, so a name that holds another as a substring does not count."
    (= 1
       ((form file
              (string "(instr(' ' || " column " || ' ', ' " word
                      " ') > 0) AS held")) :held)))

  (eprintln "scenario: a form that writes a file records the capabilities it reaches")
  (each [column word] [["caps" "io"] ["caps" "fs"] ["signal" "error"]
                       ["signal" "io"]]
    (assert (holds? "rich.lisp" column word)
            (string "a form that opens a port records " word " in " column)))
  (assert (not (holds? "rich.lisp" "caps" "error"))
          "error is a signal, not a capability")

  # The counter-factual stopped the runner's list at `os-signal`, so a form
  # that read the pid recorded an empty set: a purity the compiler never proved.
  (eprintln "scenario: a form that reads the pid records ambient")
  (assert (= ((form "ambient.lisp" "caps") :caps) "ambient")
          "a form whose one capability is the pid records ambient alone")

  # A call the compiler cannot resolve reaches every capability. The
  # counter-factual recorded seven names, and a bit added after them never
  # reached the column.
  (eprintln "scenario: an unresolved call records every built-in capability")
  (let [caps ((form "unresolved.lisp" "caps") :caps)]
    (assert (= caps built-in-caps)
            (string "the row names every built-in capability, " built-in-caps
                    ", got " caps)))

  (eprintln "scenario: a form touches what it calls and not what it defines")
  (each word ["port/open" "port/write" "slurp"]
    (assert (holds? "rich.lisp" "touches" word)
            (string "the form calls " word ", so its row must say so")))
  (assert (not (holds? "rich.lisp" "touches" "write-it"))
          "the form defines write-it, so it does not touch it")

  (eprintln "scenario: a pure form records an empty capability set")
  (let [row (form "pure.lisp" "caps, signal")]
    (assert (= row:caps "")
            (string "a form reaching nothing records an empty set, not NULL: "
                    row))
    (assert (= row:signal "error")
            (string "an assert can raise, so the form's signal is error: " row)))

  # Both files ran in this one run, so the contrast is the assertion: the NULL
  # is this file's answer, not a runner that analyzed nothing.
  (eprintln "scenario: a file the analysis refuses records NULL and still runs")
  (let [row (form "refused.lisp" "caps, touches, signal")]
    (assert (and (nil? row:caps) (nil? row:touches) (nil? row:signal))
            (string "an analysis that does not answer records NULL in all "
                    "three: " row)))
  (assert (not (nil? ((form "pure.lisp" "caps") :caps)))
          "the same run profiled the file that does analyze")
  # The run gated green above, which is the other half: a profile the
  # compiler will not produce costs the columns and nothing else.
  (assert (> (length (filter (fn [row] (= row:status "pass"))
                             (select-results db "/refused.lisp"
                               "result.status AS status"))) 0)
          "the file still runs and passes")

  (eprintln "all form-profile scenarios passed"))
