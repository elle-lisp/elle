(elle/epoch 18)
## audited: 2026-11-22
## The `--trace=residue` teardown dump: one region line per survivor, and one edge line per reference among them.
## tests/cli/overview.md
## docs/impl/region/diagnostics.md

(include-file "../runner/lib/harness.lisp")

(def header "[trace:residue] live regions after teardown: ")

(with-temp-dir dir
  (def script (write-fixture dir "script.lisp" "(+ 1 2)\n"))

  (eprintln "scenario: the residue trace dumps the surviving regions and edges")
  (let [r (elle-run ["--trace=residue" script])
        heads (filter (fn [l] (string/starts-with? l header)) (lines r:err))]
    (assert (= r:exit 0) (string "elle --trace=residue failed: " r:err))
    (assert (not (empty? heads)) (string "no residue header: " r:err))
    # The header states the count the dump then itemizes. Zero residue, the
    # end-state target, prints a bare header and no region lines, and this
    # check must keep passing on that day.
    (let [count (parse-int (slice (first heads) (length header)))
          regions (filter (fn [l]
                            (and (string/starts-with? (string/trim l) "region ")
                                 (string/contains? l " rc="))) (lines r:err))]
      (assert (= (length regions) count)
              (string "the header says " count " regions and " (length regions)
                      " region lines follow: " r:err))
      # Edge lines only ever name counted regions.
      (when (= count 0)
        (assert (not (string/contains? r:err "[trace:residue] edge "))
                (string "edges printed with zero residue: " r:err)))))

  (eprintln "scenario: the residue trace is silent when unset")
  (let [r (elle-run [script])]
    (assert (= r:exit 0) (string "elle failed: " r:err))
    (assert (not (string/contains? r:err "[trace:residue]"))
            (string "the dump printed without --trace=residue: " r:err))))

(eprintln "all trace-residue scenarios passed")
