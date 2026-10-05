(elle/epoch 14)
# audited: 2026-10-05
# Four programs on this rig under memcheck: each one's definitely-lost bytes
# and error contexts, read against the rows of tests/ledger/valgrind.lisp.
# tests/ratchet/overview.md

(def r ((import "std/ratchet")))

(let [[ok? _] (protect (subprocess/system "valgrind" ["--version"]))]
  (unless ok? (error {:error :gated :reason "valgrind is not on the path"})))
(unless (= (elle/build-profile) "release")
  (error {:error :gated
          :reason (string "the rig is a " (elle/build-profile)
                          " build, and the rows are a release build's")}))

# A definite leak counts as an error, so it raises the contexts too. Only the
# definite leaks are listed, which keeps a report to a few kilobytes.
(def memcheck
  ["--leak-check=full" "--show-leak-kinds=definite"
   "--errors-for-leak-kinds=definite"])

# [subject rig-flags source]
(def programs
  [["boot and exit" [] "(+ 1 2)"]
   ["a fiber yields and resumes" []
    "(def f (fiber/new (fn [] (yield 1) 2) :yield)) (assert (= (resume f) 1) \"the fiber yields 1\") (assert (= (resume f) 2) \"and ends with 2\")"]
   ["a file read through the I/O backend" []
    "(assert (> (length (slurp \"Cargo.toml\")) 0) \"the file has bytes\")"]
   ["a function the JIT compiles" ["--trace=syncjit"]
    "(defn step [x] (+ x 1)) (def @acc 0) (repeat 100 (assign acc (step acc))) (assert (jit? step) \"the JIT compiled step\")"]])

(defn line-with [lines marker]
  "The first of LINES that holds MARKER, or nil."
  (let [found (filter (fn [l] (string/contains? l marker)) lines)]
    (if (empty? found) nil (first found))))

(defn words-after [line marker]
  "The words of LINE that follow MARKER."
  (let [i (+ (string/find line marker) (length marker))]
    (string/split (string/trim (slice line i (length line))) " ")))

(defn count-of [word]
  "A count memcheck printed, with its thousands separators."
  (parse-int (string/replace word "," "")))

(defn lost-bytes [lines]
  "The definitely-lost bytes a report names. A report with nothing left
   allocated prints no leak summary, and says so instead."
  (let [line (line-with lines "definitely lost: ")]
    (cond
      line
        (count-of (first (words-after line "definitely lost: ")))
      (line-with lines "no leaks are possible") 0
      true (error {:error :unparsed :message "no leak summary in the report"}))))

(defn error-contexts [lines]
  "The contexts of `ERROR SUMMARY: N errors from M contexts`."
  (let [line (line-with lines "ERROR SUMMARY: ")]
    (when (nil? line)
      (error {:error :unparsed :message "no error summary in the report"}))
    (count-of (get (words-after line "ERROR SUMMARY: ") 3))))

# Every run starts before any is read, so the producer costs about one run.
(def children
  (map (fn [[subject flags source]]
         [subject
          (subprocess/exec "valgrind"
                           (concat memcheck [(elle/executable)] flags
                                   ["-e" source]) {:stdin :null :stdout :null})])
       programs))

(each [subject child] in children
  (let [report (string (port/read-all (get child :stderr)))
        code (subprocess/wait child)
        lines (string/split report "\n")]
    (assert (= code 0)
            (string subject ": the program exits 0 under memcheck\n" report))
    (r:read subject :definitely-lost (lost-bytes lines) :unit "bytes")
    (r:read subject :error-contexts (error-contexts lines) :unit "contexts")))
