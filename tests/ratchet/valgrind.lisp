(elle/epoch 14)
# audited: 2026-10-06
# Four programs under memcheck on both stdlib paths: lost bytes and error
# contexts, read against the rows of tests/ledger/valgrind.lisp.
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

# Each program's subject on each path is spelled whole, so every row of the
# ledger names a literal here (tests/integration/ledgers.rs).
(def programs
  [{:compiled "boot and exit, stdlib compiled"
    :cached "boot and exit, stdlib cached"
    :flags []
    :source "(+ 1 2)"}
   {:compiled "a fiber yields and resumes, stdlib compiled"
    :cached "a fiber yields and resumes, stdlib cached"
    :flags []
    :source "(def f (fiber/new (fn [] (yield 1) 2) :yield)) (assert (= (resume f) 1) \"the fiber yields 1\") (assert (= (resume f) 2) \"and ends with 2\")"}
   {:compiled "a file read through the I/O backend, stdlib compiled"
    :cached "a file read through the I/O backend, stdlib cached"
    :flags []
    :source "(assert (> (length (slurp \"Cargo.toml\")) 0) \"the file has bytes\")"}
   {:compiled "a function the JIT compiles, stdlib compiled"
    :cached "a function the JIT compiles, stdlib cached"
    :flags ["--trace=syncjit"]
    :source "(defn step [x] (+ x 1)) (def @acc 0) (repeat 100 (assign acc (step acc))) (assert (jit? step) \"the JIT compiled step\")"}])

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

(defn start [cache flags source]
  "One memcheck run of SOURCE on this rig, under CACHE and the rig FLAGS."
  (subprocess/exec "valgrind"
                   (concat memcheck [(elle/executable) cache] flags
                           ["-e" source]) {:stdin :null :stdout :null}))

(defn measure [dir]
  "Fill DIR's cache with one unmeasured run, then read every program on both
   paths."
  (def cached (string "--cache=" dir))
  # The cached path loads the file this run writes. Without the file, every
  # cached run would compile the standard library and read the compiled
  # path's sites under the cached path's subjects.
  (let [warm (subprocess/system (elle/executable) [cached "-e" "nil"])]
    (assert (= (get warm :exit) 0)
            (string "the unmeasured run exits 0\n" (get warm :stderr))))
  (assert (not (empty? (file/ls (path/join dir "stdlib-cache"))))
          "the unmeasured run wrote the standard library to the cache")
  # [the key of a program's subject on the path, cache-flag]. `--cache=` with
  # no directory turns caching off.
  (def paths [[:compiled "--cache="] [:cached cached]])
  # Every run starts before any is read, so the producer costs about one run.
  (def @children @[])
  (each [path cache] in paths
    (each program in programs
      (push children
            [(get program path)
             (start cache (get program :flags) (get program :source))])))
  (each [subject child] in children
    (let [report (string (port/read-all (get child :stderr)))
          code (subprocess/wait child)
          lines (string/split report "\n")]
      (assert (= code 0)
              (string subject ": the program exits 0 under memcheck\n" report))
      (r:read subject :definitely-lost (lost-bytes lines) :unit "bytes")
      (r:read subject :error-contexts (error-contexts lines) :unit "contexts"))))

(with-temp-dir dir (measure dir))
