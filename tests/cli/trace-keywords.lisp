(elle/epoch 18)
## audited: 2026-11-22
## The binary refuses an unknown trace keyword before the program runs, and lists every keyword it accepts.
## tests/cli/overview.md
## docs/config.md

(include-file "../runner/lib/harness.lisp")

# Every keyword the binary accepts, as its help lists them: the text between
# the trace flag's `Keywords:` and `--trace=all`. The help renders the
# binary's own keyword set, so a keyword the binary gains joins the list here.
(def accepted
  (let [r (elle-run ["--help"])
        text r:out
        anchor "Trace subsystems. Keywords:"
        from (+ (string/find text anchor) (length anchor))
        to (string/find text "--trace=all")]
    (assert (= r:exit 0) (string "elle --help failed: " r:err))
    (string/join (sort (filter (fn [w] (not (empty? w)))
                               (map string/trim
                                    (string/split (slice text from to) ","))))
                 ",")))

(eprintln "scenario: an unknown trace keyword fails before the program runs")
(let [r (elle-run ["--trace=call,nonsense" "-e" "(println \"ran\")"])
      marker "Valid: "
      at (string/find r:err marker)]
  (assert (not (= r:exit 0)) "an unknown trace keyword was accepted")
  (assert (empty? r:out) (string "the program ran before the refusal: " r:out))
  (assert (string/contains? r:err "--trace: unknown keyword 'nonsense'")
          (string "the refusal names the keyword: " r:err))
  (assert (not (nil? at)) (string "the refusal lists no keyword: " r:err))
  (let [listed (string/split (string/trim
                               (first (lines
                                        (slice r:err (+ at (length marker))))))
                             ", ")]
    (assert (= (string/join (sort listed) ",") accepted)
            (string "the refusal lists every keyword the binary accepts, "
                    accepted ", got " listed))))

(eprintln "all trace-keywords scenarios passed")
