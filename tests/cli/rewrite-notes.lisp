(elle/epoch 18)
## audited: 2026-11-22
## `elle rewrite` names each def the epoch 17 rule leaves, and each forward read the epoch 18 rule leaves, an included file's too.
## tests/cli/overview.md
## docs/epochs/bodies.md

(include-file "../runner/lib/harness.lisp")

# A file whose function reads `gun`.
(def lib "(defn shoot [] (gun 1))\n")

(with-temp-dir dir
  (def @n 0)

  (defn dry-run [source &opt files]
    "Write SOURCE as old.lisp in a directory of its own, with each of FILES,
     a name and its text, beside it, and run `elle rewrite --dry-run` on it.
     Answer the run, the file's path as :path, and its text after as :after."
    (assign n (+ n 1))
    (let [home (path/join dir (string "case-" n))]
      (file/mkdir-all home)
      (each [name text] (or files [])
        (write-fixture home name text))
      (let [path (write-fixture home "old.lisp" source)
            r (elle-run ["rewrite" "--dry-run" path])]
        (assert (= r:exit 0) (string "a note is no error; stderr: " r:err))
        (put (put r :path path) :after (slurp path)))))

  (defn silent [source what &opt files]
    (let [r (dry-run source files)]
      (assert (empty? r:err) (string what ": " r:err))))

  # A read before the form reaches the def, so the rule neither moves it into
  # a let nor assigns it.
  (eprintln "scenario: a def the rule leaves is named by line and column")
  (let [source "(elle/epoch 16)\n(defn f [c]\n  (g x)\n  (when c (def x 1)))\n"
        r (dry-run source)]
    (assert (string/contains? r:err (string r:path ":4:11"))
            (string "the note names the def where it stands: " r:err))
    (assert (= r:after source) "a dry run writes nothing"))

  (eprintln "scenario: a def the rule moves is not named")
  (silent "(elle/epoch 16)\n(defn f [c] (when c (def x 1) (g x)))\n"
          "a moved def needs no note")

  (eprintln "scenario: a forward read the rule leaves is named by line and column")
  (let [source "(elle/epoch 17)\n(defn f [] (g))\n(println 1)\n(defn g [] 1)\n"
        r (dry-run source)]
    (assert (and (string/contains? r:err (string r:path ":2:13"))
                 (string/contains? r:err "'g'"))
            (string "the note names the read where it stands: " r:err))
    (assert (= r:after source) "a dry run writes nothing"))

  (eprintln "scenario: a forward read the rule carries is not named")
  (silent "(elle/epoch 17)\n(defn f [] (g))\n(defn g [] 1)\n(f)\n"
          "a carried read needs no note")

  # The counter-factual named nothing, and the compiler then refused the read
  # as an undefined variable inside the included file.
  (eprintln "scenario: an included read of a later def is named at the include")
  (let [source "(elle/epoch 17)\n(include-file \"lib.lisp\")\n(def gun (make-gun))\n(shoot)\n"
        r (dry-run source [["lib.lisp" lib]])]
    (assert (and (string/contains? r:err (string r:path ":2:1"))
                 (string/contains? r:err "'gun'"))
            (string "the note names the include and the name: " r:err))
    (assert (string/contains? r:err "lib.lisp:1:17")
            (string "the note names where the read stands: " r:err))
    (assert (= r:after source) "a dry run writes nothing"))

  (eprintln "scenario: an included read in a body is named at its include")
  (let [r (dry-run (string "(elle/epoch 17)\n(defn outer []\n  (include-file \"lib.lisp\")\n"
                           "  (def gun (make-gun))\n  (shoot))\n")
                   [["lib.lisp" lib]])]
    (assert (and (string/contains? r:err (string r:path ":3:3"))
                 (string/contains? r:err "'gun'"))
            (string "the note names the include in the body: " r:err)))

  (eprintln "scenario: an included read of an earlier def is not named")
  (silent "(elle/epoch 17)\n(def gun (make-gun))\n(include-file \"lib.lisp\")\n(shoot)\n"
          "the read finds the def" [["lib.lisp" lib]])

  (eprintln "scenario: a name the included file defines is not named")
  (silent "(elle/epoch 17)\n(include-file \"lib.lisp\")\n(def gun (make-gun))\n(shoot)\n"
          "the read finds the file's own def"
          [["lib.lisp" "(def gun (fn [n] n))\n(defn shoot [] (gun 1))\n"]])

  # The compile reports the missing file; the rewrite has nothing to read.
  (eprintln "scenario: an include of a file that is not there is not named")
  (silent "(elle/epoch 17)\n(include-file \"gone.lisp\")\n(def gun (make-gun))\n"
          "a missing include needs no note")

  (eprintln "scenario: a read in a file an included file includes is named at the outer include")
  (let [r (dry-run "(elle/epoch 17)\n(include-file \"lib.lisp\")\n(def gun (make-gun))\n(shoot)\n"
                   [["lib.lisp" "(include-file \"deep.lisp\")\n"]
                    ["deep.lisp" lib]])]
    (assert (and (string/contains? r:err (string r:path ":2:1"))
                 (string/contains? r:err "deep.lisp:1:17"))
            (string "the note names the outer include and the read: " r:err)))

  # The counter-factual read the file again at each include of it, and never
  # ended.
  (eprintln "scenario: an included file that includes itself is read once")
  (let [r (dry-run "(elle/epoch 17)\n(include-file \"lib.lisp\")\n(def gun (make-gun))\n(shoot)\n"
                   [["lib.lisp"
                     "(include-file \"lib.lisp\")\n(defn shoot [] (gun 1))\n"]])]
    (assert (= (length (string/split r:err "lib.lisp:2:17")) 2)
            (string "one note names the read: " r:err))))

(eprintln "all rewrite-notes scenarios passed")
