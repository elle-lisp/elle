(elle/epoch 14)
# audited: 2026-10-06
# import/resolve finds the file a spec names, and the import macro resolves ./ specs against the file that writes them.
# docs/modules.md

(def here (path/parent (get (meta/location) :file)))
(def root (path/normalize (path/join here "../..")))

(defn resolves? [spec & dir]
  "Whether import/resolve answers spec, given dir when there is one."
  (first (protect (import/resolve spec ;dir))))

# ── The virtual prefixes ────────────────────────────────────────────────

(assert (= (path/join root "lib/base64.lisp") (import/resolve "std/base64"))
        "std/X names lib/X.lisp under the project root")
(assert (not (resolves? "std/no-such-module"))
        "a std/ spec with no file names nothing")
(let [[ok? err] (protect (import/resolve "std/no-such-module"))]
  (assert (= :io-error (get err :error))
          "a spec that names nothing raises :io-error"))

# ── ./ and ../ name a file relative to dir, and nowhere else ────────────

(assert (= (path/join here "import-resolve.lisp")
           (import/resolve "./import-resolve" here))
        "a ./ spec tries .lisp in dir")
(assert (= (path/join here "import-resolve.lisp")
           (import/resolve "./import-resolve.lisp" here))
        "a ./ spec names a file as it stands")
(assert (= (path/join root "tests/modules/test.lisp")
           (import/resolve "../modules/test" here))
        "a ../ spec resolves against dir, and the result is normalized")
(assert (not (resolves? "./import-resolve"))
        "without dir, a ./ spec names nothing")

# ── A bare spec searches --path and home, never dir ─────────────────────

# The counter-factual searches dir first, and a file beside the program then
# shadows a library of the same name.
(assert (not (resolves? "import-resolve" here))
        "a bare spec does not search dir")

# ── An absolute spec ────────────────────────────────────────────────────

(assert (= (path/join root "tests/modules/test.lisp")
           (import/resolve (path/join root "tests/modules/test")))
        "an absolute spec is tried with the same probes")

# ── What import/resolve reads ───────────────────────────────────────────

(assert (array? (vm/config :path)) "the search path is an array")
(assert (contains? |"so" "dylib" "dll"| (vm/config :plugin-suffix))
        "the plugin suffix is the platform's")
(let [home (vm/config :home)]
  (assert (or (nil? home) (string? home)) "home is a directory or nil"))

# ── The import macro ────────────────────────────────────────────────────

(let [m ((import "../modules/test"))]
  (assert (= 42 m:test-var)
          "import resolves a ../ spec against the file that writes it"))
(let [b64 ((import "std/base64"))]
  (assert (= "aGVsbG8=" (b64:encode "hello")) "import resolves std/"))
(let [load (fn [spec] (import spec))
      m ((load "../modules/test"))]
  (assert (= 42 m:test-var) "import works on a computed spec inside a function"))
