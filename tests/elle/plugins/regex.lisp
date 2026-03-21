(elle/epoch 1)

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
  (display "SKIP: regex plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def compile-fn      (get plugin :compile))
(def match-fn        (get plugin :match?))
(def find-fn         (get plugin :find))
(def find-all-fn     (get plugin :find-all))
(def captures-fn     (get plugin :captures))
(def captures-all-fn (get plugin :captures-all))
(def replace-fn      (get plugin :replace))
(def replace-all-fn  (get plugin :replace-all))
(def split-fn        (get plugin :split))

# ── regex/compile ──────────────────────────────────────────────────

(assert (not (nil? (compile-fn "\\d+"))) "regex/compile valid pattern")

(let (([ok? _] (protect ((fn () (compile-fn "[invalid")))))) (assert (not ok?) "regex/compile invalid pattern"))

(let (([ok? _] (protect ((fn () (compile-fn 42)))))) (assert (not ok?) "regex/compile wrong type"))

(let (([ok? _] (protect ((fn () (compile-fn)))))) (assert (not ok?) "regex/compile wrong arity: no args"))

(let (([ok? _] (protect ((fn () (compile-fn "a" "b")))))) (assert (not ok?) "regex/compile wrong arity: two args"))

# ── regex/match? ───────────────────────────────────────────────────

(assert (match-fn (compile-fn "\\d+") "abc123") "regex/match? true")

(assert (not (match-fn (compile-fn "\\d+") "abc")) "regex/match? false")

(let (([ok? _] (protect ((fn () (match-fn "not-a-regex" "abc")))))) (assert (not ok?) "regex/match? wrong type"))

# ── regex/find ─────────────────────────────────────────────────────

(assert (= (get (find-fn (compile-fn "\\d+") "abc123def") :match) "123") "regex/find match value")

(let ((m (find-fn (compile-fn "\\d+") "abc123def")))
  (assert (= (get m :start) 3) "regex/find start")
  (assert (= (get m :end) 6) "regex/find end"))

(assert (= (find-fn (compile-fn "\\d+") "abc") nil) "regex/find no match returns nil")

(let (([ok? _] (protect ((fn () (find-fn (compile-fn "x"))))))) (assert (not ok?) "regex/find wrong arity"))

# ── regex/find-all ─────────────────────────────────────────────────

(assert (= (length (find-all-fn (compile-fn "\\d+") "a1b22c333")) 3) "regex/find-all multiple matches count")

(assert (= (get (first (find-all-fn (compile-fn "\\d+") "a1b22c333")) :match) "1") "regex/find-all first match value")

(assert (empty? (find-all-fn (compile-fn "\\d+") "abc")) "regex/find-all no matches")

# ── regex/captures ─────────────────────────────────────────────────

(let ((c (captures-fn (compile-fn "(\\d+)-(\\w+)") "42-hello")))
  (assert (= (get c :0) "42-hello") "regex/captures group 0: full match")
  (assert (= (get c :1) "42") "regex/captures group 1")
  (assert (= (get c :2) "hello") "regex/captures group 2"))

(let ((c (captures-fn
            (compile-fn "(?P<year>\\d{4})-(?P<month>\\d{2})")
            "2024-01-15")))
  (assert (= (get c :year) "2024") "regex/captures named: year")
  (assert (= (get c :month) "01") "regex/captures named: month"))

(assert (= (captures-fn (compile-fn "\\d+") "abc") nil) "regex/captures no match returns nil")

(let (([ok? _] (protect ((fn () (captures-fn (compile-fn "x"))))))) (assert (not ok?) "regex/captures wrong arity"))

# ── regex/captures-all ─────────────────────────────────────────────

(let ((results (captures-all-fn (compile-fn "(\\d+)-(\\w+)") "1-a 2-b 3-c")))
  (assert (= (length results) 3) "regex/captures-all count")
  (assert (= (get (first results) :1) "1") "regex/captures-all first group 1")
  (assert (= (get (first results) :2) "a") "regex/captures-all first group 2"))

(assert (empty? (captures-all-fn (compile-fn "\\d+") "abc")) "regex/captures-all no matches")

# ── regex/replace ──────────────────────────────────────────────────

(assert (= (replace-fn (compile-fn "\\d+") "a1b2c3" "N") "aNb2c3") "regex/replace first only")

(assert (= (replace-fn (compile-fn "\\d+") "abc" "N") "abc") "regex/replace no match unchanged")

(assert (= (replace-fn (compile-fn "(\\d+)") "val=42" "[$1]") "val=[42]") "regex/replace backreference")

(let (([ok? _] (protect ((fn () (replace-fn (compile-fn "x") "abc")))))) (assert (not ok?) "regex/replace wrong arity"))

# ── regex/replace-all ──────────────────────────────────────────────

(assert (= (replace-all-fn (compile-fn "\\d+") "a1b2c3" "N") "aNbNcN") "regex/replace-all all matches")

(assert (= (replace-all-fn (compile-fn "\\d+") "abc" "N") "abc") "regex/replace-all no match unchanged")

# ── regex/split ────────────────────────────────────────────────────

(let ((parts (split-fn (compile-fn "[,;]+") "a,b;;c")))
  (assert (= (length parts) 3) "regex/split count")
  (assert (= (first parts) "a") "regex/split first")
  (assert (= (last parts) "c") "regex/split last"))

(assert (= (length (split-fn (compile-fn ",") "abc")) 1) "regex/split no delimiter")

(let (([ok? _] (protect ((fn () (split-fn (compile-fn ","))))))) (assert (not ok?) "regex/split wrong arity"))
