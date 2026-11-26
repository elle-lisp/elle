(elle/epoch 18)
# audited: 2026-11-25
# Four programs under memcheck on both stdlib paths: lost bytes and error
# contexts, read against the rows of tests/ledger/valgrind.lisp.
#
# tests/ratchet/overview.md

(def r ((import "std/ratchet")))

# The profile first, so a debug rig gives one reason with valgrind or without.
(unless (= (elle/build-profile) "release")
  (error {:error :gated
          :reason (string "the rig is a " (elle/build-profile)
                          " build, and the rows are a release build's")}))
(let [[ok? _] (protect (subprocess/system "valgrind" ["--version"]))]
  (unless ok? (error {:error :gated :reason "valgrind is not on the path"})))

# A definite leak counts as an error, so it raises the contexts too. Only the
# definite leaks are listed, which keeps a report to a few kilobytes.
(def memcheck
  ["--leak-check=full" "--show-leak-kinds=definite"
   "--errors-for-leak-kinds=definite"])

# Each program's subject on each path is spelled whole, so every row of the
# ledger names a literal here (tests/integration/ledgers.rs).
#
# The JIT program's function is recursive, so the graft keeps its call. Under
# the graft, a call to a function the compiler can see becomes that function's
# body. A function nothing calls is never compiled, so `jit?` would answer false.
(def programs
  [{:compiled "boot and exit, stdlib compiled"
    :hydrated "boot and exit, stdlib hydrated"
    :flags []
    :source "(+ 1 2)"}
   {:compiled "a fiber yields and resumes, stdlib compiled"
    :hydrated "a fiber yields and resumes, stdlib hydrated"
    :flags []
    :source "(def f (fiber/new (fn [] (yield 1) 2) :yield)) (assert (= (resume f) 1) \"the fiber yields 1\") (assert (= (resume f) 2) \"and ends with 2\")"}
   {:compiled "a file read through the I/O backend, stdlib compiled"
    :hydrated "a file read through the I/O backend, stdlib hydrated"
    :flags []
    :source "(assert (> (length (slurp \"Cargo.toml\")) 0) \"the file has bytes\")"}
   {:compiled "a function the JIT compiles, stdlib compiled"
    :hydrated "a function the JIT compiles, stdlib hydrated"
    :flags ["--trace=syncjit"]
    :source "(defn step [x n] (if (= n 0) x (step (+ x 1) (- n 1)))) (def @acc 0) (repeat 100 (assign acc (step acc 1))) (assert (jit? step) \"the JIT compiled step\")"}])

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

(defn start [boot flags source]
  "One memcheck run of SOURCE on this rig, booted under BOOT and the rig FLAGS."
  (subprocess/exec "valgrind"
                   (concat memcheck [(elle/executable)] boot flags ["-e" source])
                   {:stdin :null :stdout :null}))

(defn measure [dir]
  "Store a boot image in DIR with one unmeasured run, then read every program
   on both paths."
  # `--boot-image=DIR` reads the warm cache in DIR and never the blob a release
  # rig carries, so the hydrated path reads the image this run stores.
  (def warm-cache [(string "--boot-image=" dir)])
  (let [warm (subprocess/system (elle/executable)
                                (concat warm-cache ["-e" "nil"]))]
    (assert (= (get warm :exit) 0)
            (string "the unmeasured run exits 0\n" (get warm :stderr))))
  (assert (any? (fn [n] (string/ends-with? n ".image")) (file/ls dir))
          "the unmeasured run stored a boot image")
  # [the key of a program's subject on the path, its boot flags]. `--cache=`
  # with no directory turns the disk caches off, and `--boot-image=off` turns
  # off the blob a release rig carries, so the compiled path compiles. A
  # hydrated run traces its boot, so the mark below proves it hydrated.
  (def paths
    [[:compiled ["--cache=" "--boot-image=off"]]
     [:hydrated [;warm-cache "--trace=boot"]]])
  # Every run starts before any is read, so the producer costs about one run.
  (def @children @[])
  (each [path boot] in paths
    (each program in programs
      (push children
            [path (get program path)
             (start boot (get program :flags) (get program :source))])))
  (each [path subject child] in children
    (let [report (string (port/read-all (get child :stderr)))
          code (subprocess/wait child)
          lines (string/split report "\n")]
      (assert (= code 0)
              (string subject ": the program exits 0 under memcheck\n" report))
      # Without the mark, a hydrated run that compiled would read the compiled
      # path's sites under the hydrated path's subjects.
      (when (= path :hydrated)
        (assert (line-with lines "image-hydrate")
                (string subject ": the run hydrated no image\n" report)))
      (r:read subject :definitely-lost (lost-bytes lines) :unit "bytes")
      (r:read subject :error-contexts (error-contexts lines) :unit "contexts"))))

(with-temp-dir dir (measure dir))
