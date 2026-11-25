(elle/epoch 18)
## audited: 2026-11-22
## Where import/resolve looks for a spec: the working directory, the path, home, and the writer's directory.
## tests/cli/overview.md
## docs/modules/resolution.md
## docs/config.md
##
## Each scenario writes its modules into a directory of its own and runs elle
## from a directory it picks, so the working directory is part of every case.

(include-file "../runner/lib/harness.lisp")
(include-file "lib/modules.lisp")

(defn prints [cwd args input want what]
  "Run the binary in CWD, and require exit 0 and WANT on stdout."
  (let [r (run-in cwd args input)]
    (assert (= r:exit 0) (string what "; stderr: " r:err))
    (assert (= (string/trim r:out) want)
            (string what ": printed " r:out ", wanted " want))))

(with-temp-dir dir
  (defn case-dir [name]
    (let [root (path/join dir name)]
      (file/mkdir-all root)
      root))

  # The counter-factual searched the path and home alone, and failed with an
  # `:io-error`.
  (eprintln "scenario: a bare spec searches the working directory")
  (let [root (case-dir "bare")]
    (write-module root "m.lisp" "42")
    (prints root ["-"] "(print (import \"m\"))" "42" "m.lisp in the cwd loads"))

  # A bare spec that names a directory, as `"lib/redis.lisp"` does.
  (eprintln "scenario: a bare spec with a directory searches the working directory")
  (let [root (case-dir "subdir")]
    (write-module root "sub/m.lisp" "7")
    (prints root ["-"] "(print (import \"sub/m.lisp\"))" "7" "sub/m.lisp loads"))

  # The counter-factual searched the path first and printed 2.
  (eprintln "scenario: the working directory comes before the path")
  (let [root (case-dir "order")]
    (write-module root "m.lisp" "1")
    (write-module root "lib/m.lisp" "2")
    (prints root ["--path=lib" "-"] "(print (import \"m\"))" "1"
            "the working directory's m wins"))

  # A `std/` spec resolves before the search, so a file of that name in the
  # working directory does not shadow the standard library.
  (eprintln "scenario: a std spec is not shadowed by the working directory")
  (let [root (case-dir "std")]
    (write-module root "std/base64.lisp" "42")
    (prints root ["-"]
            "(let [b ((import \"std/base64\"))] (print (b:encode \"hi\")))"
            "aGk=" "the library's base64 answered"))

  # A relative entry resolves against the working directory. The program runs
  # from a sibling directory, so only the `--path` entry can find the module.
  (eprintln "scenario: --path makes a bare spec resolve")
  (let [root (case-dir "path")
        elsewhere (case-dir "path/elsewhere")]
    (write-module root "lib/m.lisp" "42")
    (prints elsewhere [(string "--path=" (path/join root "lib")) "-"]
            "(print (import \"m\"))" "42" "an absolute --path entry finds m")
    (prints elsewhere ["--path=../lib" "-"] "(print (import \"m\"))" "42"
            "a relative --path entry finds m"))

  # A program with no file has no directory of its own. The counter-factual
  # named nothing.
  (eprintln "scenario: a relative spec on stdin resolves against the working directory")
  (let [root (case-dir "stdin")]
    (write-module root "m.lisp" "42")
    (prints root ["-"] "(print (import \"./m\"))" "42" "./m loads"))

  # The program lives in prog/ and runs from the root, where m.lisp is. The
  # counter-factual looked in prog/ alone.
  (eprintln "scenario: a relative spec falls back to the working directory")
  (let [root (case-dir "fallback")]
    (write-module root "m.lisp" "42")
    (write-module root "prog/main.lisp" "(print (import \"./m\"))")
    (prints root ["prog/main.lisp"] "" "42" "./m falls back to the root"))

  (eprintln "scenario: a relative spec prefers the writer's directory")
  (let [root (case-dir "writer")]
    (write-module root "m.lisp" "2")
    (write-module root "prog/m.lisp" "1")
    (write-module root "prog/main.lisp" "(print (import \"./m\"))")
    (prints root ["prog/main.lisp"] "" "1" "the writer's m wins")))

(eprintln "all module-search scenarios passed")
