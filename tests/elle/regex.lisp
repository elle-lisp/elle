(elle/epoch 8)

## Regex plugin integration tests
## Tests the regex plugin (.so loaded via import-file)
## Migrated from tests/integration/regex.rs
##
## Plugin symbols (regex/compile, regex/match?, etc.) are only available at
## runtime after import-file loads the .so. Because file-as-letrec compiles
## the entire file before executing any of it, we use the struct returned by
## import-file to access plugin functions.

## Try to load the regex plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_regex.so")))
(when (not ok?)
  (print "SKIP: regex plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def compile-fn  (get plugin :compile))
(def match-fn    (get plugin :match?))
(def find-fn     (get plugin :find))
(def find-all-fn (get plugin :find-all))
(def captures-fn (get plugin :captures))

# ── regex/compile ──────────────────────────────────────────────────

(assert (not (nil? (compile-fn "\\d+"))) "regex/compile valid pattern")

(let [[ok? _] (protect ((fn () (compile-fn "[invalid"))))] (assert (not ok?) "regex/compile invalid pattern"))

(let [[ok? _] (protect ((fn () (compile-fn 42))))] (assert (not ok?) "regex/compile wrong type"))

(let [[ok? _] (protect ((fn () (compile-fn))))] (assert (not ok?) "regex/compile wrong arity: no args"))

(let [[ok? _] (protect ((fn () (compile-fn "a" "b"))))] (assert (not ok?) "regex/compile wrong arity: two args"))

# ── regex/match? ───────────────────────────────────────────────────

(assert (match-fn (compile-fn "\\d+") "abc123") "regex/match? true")

(assert (not (match-fn (compile-fn "\\d+") "abc")) "regex/match? false")

(let [[ok? _] (protect ((fn () (match-fn "not-a-regex" "abc"))))] (assert (not ok?) "regex/match? wrong type"))

# ── regex/find ─────────────────────────────────────────────────────

(assert (= (get (find-fn (compile-fn "\\d+") "abc123def") :match) "123") "regex/find match value")

(let [m (find-fn (compile-fn "\\d+") "abc123def")]
  (assert (= (get m :start) 3) "regex/find start")
  (assert (= (get m :end) 6) "regex/find end"))

(assert (= (find-fn (compile-fn "\\d+") "abc") nil) "regex/find no match returns nil")

(let [[ok? _] (protect ((fn () (find-fn (compile-fn "x")))))] (assert (not ok?) "regex/find wrong arity"))

# ── regex/find-all ─────────────────────────────────────────────────

(assert (= (length (find-all-fn (compile-fn "\\d+") "a1b22c333")) 3) "regex/find-all multiple matches count")

(assert (= (get (first (find-all-fn (compile-fn "\\d+") "a1b22c333")) :match) "1") "regex/find-all first match value")

(assert (empty? (find-all-fn (compile-fn "\\d+") "abc")) "regex/find-all no matches")

# ── regex/captures ─────────────────────────────────────────────────

(let [c (captures-fn (compile-fn "(\\d+)-(\\w+)") "42-hello")]
  (assert (= (get c :0) "42-hello") "regex/captures group 0: full match")
  (assert (= (get c :1) "42") "regex/captures group 1")
  (assert (= (get c :2) "hello") "regex/captures group 2"))

(let [c (captures-fn
            (compile-fn "(?P<year>\\d{4})-(?P<month>\\d{2})")
            "2024-01-15")]
  (assert (= (get c :year) "2024") "regex/captures named: year")
  (assert (= (get c :month) "01") "regex/captures named: month"))

(assert (= (captures-fn (compile-fn "\\d+") "abc") nil) "regex/captures no match returns nil")

(let [[ok? _] (protect ((fn () (captures-fn (compile-fn "x")))))] (assert (not ok?) "regex/captures wrong arity"))
