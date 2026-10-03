(elle/epoch 12)
# audited: 2026-09-21
# The (elle/version) and (elle/migration) declarations are consumed at
# compile time, and a malformed declaration refuses to compile.
#
# The counter-factual: without the front-end strip, a version declaration
# is an arity error on the 0-ary elle/version primitive and a migration
# form is an undefined variable — a versioned module could not load.

# A module file carrying both declarations imports, constructs, and runs.
(with-temp-dir dir
               (let [path (path/join dir "mod.lisp")]
                 (file/write path
                             (string "(elle/epoch 12)\n"
                                     "(elle/version \"2.1.3\")\n"
                                     "(elle/migration 2\n" "  \"one summary\"\n"
                                     "  (rename old-name new-name)\n"
                                     "  (replace (bump $1 $2) (increment $2 $1))\n"
                                     "  (remove gone \"use something else\")\n"
                                     "  (warn kept \"kept but louder\"))\n"
                                     "(fn [] {:answer (fn [] 42)})\n"))
                 (let [mod ((import-file path))]
                   (assert (= ((get mod :answer)) 42)
                           "the versioned module runs"))))

# The declarations vanish before analysis: an analysis of a versioned
# module reports its exports, not an arity error.
(let [ex (compile/exports (compile/analyze (string "(elle/version \"1.0.0\")\n"
                          "(fn [] {:f (fn [x] x)})")))]
  (assert (= (get (get (get ex :exports) :f) :required) 1)
          "analysis sees through the declaration"))

# The 0-ary query keeps its meaning: the interpreter's own version.
(assert (string? (elle/version)) "the query form still answers")

# A second version declaration refuses to compile.
(let [[ok? _] (protect (compile/analyze (string "(elle/version \"1.0.0\")\n"
                                        "(elle/version \"1.0.1\")\n" "nil")))]
  (assert (not ok?) "duplicate version declarations"))

# A non-literal version refuses to compile.
(let [[ok? _] (protect (compile/analyze "(elle/version (string \"1\")) nil"))]
  (assert (not ok?) "version must be a literal string"))

# A migration rule outside the vocabulary refuses to compile.
(let [[ok? _] (protect (compile/analyze "(elle/migration 2 (explode f)) nil"))]
  (assert (not ok?) "unknown migration rule"))

# A migration form needs a positive major.
(let [[ok? _] (protect (compile/analyze "(elle/migration 0 (rename a b)) nil"))]
  (assert (not ok?) "major zero is not a migration"))

# Two migration forms for one major refuse to compile.
(let [[ok? _] (protect (compile/analyze (string "(elle/migration 2 (rename a b))\n"
                                        "(elle/migration 2 (rename c d))\n"
                                        "nil")))]
  (assert (not ok?) "one form per major"))

# Malformed rule shapes refuse to compile.
(let [[ok? _] (protect (compile/analyze "(elle/migration 2 (rename a)) nil"))]
  (assert (not ok?) "rename wants two symbols"))
(let [[ok? _] (protect (compile/analyze "(elle/migration 2 (remove gone)) nil"))]
  (assert (not ok?) "remove wants a message"))

(println "semver-version-form: all tests passed")
