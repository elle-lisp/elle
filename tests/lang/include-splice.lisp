(elle/epoch 15)
# audited: 2026-10-22
# An include splices its file's forms into the form that holds it, and each form sees the names around the include.
#
# docs/include.md
#
# Each case writes its files into a directory of its own and loads main.lisp
# there, so a compile that fails fails its own case. Before an include spliced
# into the form that held it, each case failed. Most failed on `undefined
# variable: include-file`. The top-level cases failed on a name the included
# code could not see, on a second include refused as circular, or on a cycle
# error that named one file.

# Write each [name text] of `files` into a fresh directory, load main.lisp
# there, and answer what `protect` answers: [ok? value-or-error].
(defn load-program [files]
  (with-temp-dir dir
                 (each [name text] files
                   (file/write (path/join dir name) text))
                 (protect (import-file (path/join dir "main.lisp")))))

(defn loads-to [expected files what]
  (let [[ok? v] (load-program files)]
    (assert ok?
            (string/join [what ": " (if (struct? v) (get v :message) "")] ""))
    (assert (= expected v) what)))

(def scale-file ["inc.lisp" "(defn scale [v] (* v factor))\n"])

# ── Where an include splices ─────────────────────────────────────────────

(loads-to 30
          [scale-file
           ["main.lisp"
            "(defn scaled [factor x] (include-file \"inc.lisp\") (scale x))\n(scaled 3 10)\n"]]
          "a function body takes the forms, and they read its parameter")

(loads-to 10
          [scale-file
           ["main.lisp"
            "(let [factor 2] (include-file \"inc.lisp\") (scale 5))\n"]]
          "a let body takes the forms")

# The included `ev?` calls `od?`, which the body defines after the include.
(loads-to [true false]
          [["inc.lisp" "(defn ev? [n] (if (= n 0) true (od? (- n 1))))\n"]
           ["main.lisp"
            "(defn parity [n]\n  (include-file \"inc.lisp\")\n  (defn od? [n] (if (= n 0) false (ev? (- n 1))))\n  (ev? n))\n[(parity 10) (parity 7)]\n"]]
          "an included definition and a later body definition call each other")

(loads-to 42
          [["inc.lisp" "41\n"]
           ["main.lisp" "(+ 1 (include-file \"inc.lisp\"))\n"]]
          "an argument position takes the file's one form")

(loads-to [0 1 2 3]
          [["inc.lisp" "1 2\n"]
           ["main.lisp" "[0 (include-file \"inc.lisp\") 3]\n"]]
          "an array literal takes each form as an element")

(loads-to 42
          [["inc.lisp" "41\n"]
           ["main.lisp"
            "(defmacro forty-one [] `(include-file \"inc.lisp\"))\n(+ 1 (forty-one))\n"]]
          "a macro that expands to an include splices where the call stood")

# ── What an included form sees ───────────────────────────────────────────

(loads-to 11
          [["inc.lisp" "(def from-base (+ base 1))\n"]
           ["main.lisp"
            "(def base 10)\n(include-file \"inc.lisp\")\nfrom-base\n"]]
          "included code at the top level reads the including file's definition")

# The template's include defines a `hidden` that the template reads and the
# call site does not. Included forms that took no scope from the include
# would read the call site's `hidden`, and answer 101.
(loads-to [2 100]
          [["inc.lisp" "(def hidden 1)\n"]
           ["main.lisp"
            "(def hidden 100)\n(defmacro hidden-plus-one [] `(begin (include-file \"inc.lisp\") (+ hidden 1)))\n[(hidden-plus-one) hidden]\n"]]
          "an include a template writes is hygienic")

(let [[ok? v] (load-program [["inc.lisp" "(meta/location)\n"]
                             ["main.lisp"
                              "(defn where [] (include-file \"inc.lisp\"))\n(where)\n"]])]
  (assert (and ok? (= "inc.lisp" (path/filename (get v :file))))
          "an included form keeps the location of the file it came from"))

# ── Repeats and cycles ───────────────────────────────────────────────────

(loads-to [3 10]
          [scale-file
           ["main.lisp"
            "(defn f [factor] (include-file \"inc.lisp\") (scale 1))\n(defn g [factor] (include-file \"inc.lisp\") (scale 2))\n[(f 3) (g 5)]\n"]]
          "two bodies include one file")

# The counter-factual kept every file a compile had included, and refused the
# second include as circular.
(loads-to 41
          [["inc.lisp" "41\n"]
           ["main.lisp"
            "(include-file \"inc.lisp\")\n(include-file \"inc.lisp\")\n"]]
          "a file included twice at the top level is no cycle")

(with-temp-dir dir
               (let [a (path/join dir "a.lisp")
                     b (path/join dir "b.lisp")]
                 (file/write a "(include-file \"b.lisp\")\n")
                 (file/write b "(defn f [] (include-file \"a.lisp\"))\n")
                 (let [[ok? err] (protect (import-file a))
                       chain (string/join [a " -> " b " -> " a] "")]
                   (assert (not ok?) "a cycle through a body include fails")
                   (assert (string/contains? (get err :message) chain)
                           (string/join ["the error names the chain " chain ": "
                                        (get err :message)] "")))))

# ── Which compiles splice ────────────────────────────────────────────────

(with-temp-dir dir
               (let [one (path/join dir "one.lisp")
                     two (path/join dir "two.lisp")
                     lib (path/join dir "lib.lisp")]
                 (file/write one "41\n")
                 (file/write two "1 2\n")
                 (file/write lib "(defn twice [v] (* 2 v))\n")
                 (assert (= [true 41] (protect (eval (list 'include-file one))))
                         "eval splices an include of one form")
                 (assert (= [true 2] (protect (eval (list 'include-file two))))
                         "eval takes two forms in a begin, which answers the last")
                 (let [src (string/join ["(include-file \"" lib
                                        "\")\n(defn g [] (twice 1))\n"] "")
                       a (compile/analyze src
                       {:file (path/join dir "main.lisp")})
                       [ok? sig] (protect (compile/signal a :twice))]
                   (assert ok? "compile/analyze sees an included definition")
                   (assert (get sig :silent) "and infers its signal"))))

# A body include reads under the capabilities of the fiber the compile runs
# for, as a top-level one does.
(with-temp-dir dir
               (let [inc (path/join dir "inc.lisp")
                     form (read (string/join ["(fn [] (include-file \"" inc
                                "\"))"] ""))]
                 (file/write inc "42\n")
                 (let [v (fiber/resume (fiber/new (fn []
                                         (import/load-syntax form)) |:fs :error|
                                       :deny |:fs|))]
                   (assert (string/contains? (get v :message) "needs :fs")
                           "a body include under :deny |:fs| reads nothing"))
                 (assert (= [true 42] (protect ((import/load-syntax form))))
                         "and an unrestricted compile reads it")))

# The store keeps a unit while its includes read the same, a body's included.
(with-temp-dir dir
               (let [main (path/join dir "main.lisp")
                     inc (path/join dir "inc.lisp")]
                 (file/write inc "(defn answer [] 1)\n")
                 (file/write main
                             "(defn f [] (include-file \"inc.lisp\") (answer))\n(f)\n")
                 (assert (= [true 1] (protect (import-file main)))
                         "the body include's text runs")
                 (file/write inc "(defn answer [] 2)\n")
                 (assert (= [true 2] (protect (import-file main)))
                         "the edited body include's text runs")))

(println "include-splice: OK")
