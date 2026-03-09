(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

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
(def compile-fn  (get plugin :compile))
(def match-fn    (get plugin :match?))
(def find-fn     (get plugin :find))
(def find-all-fn (get plugin :find-all))
(def captures-fn (get plugin :captures))

# ── regex/compile ──────────────────────────────────────────────────

(assert-true
  (not (nil? (compile-fn "\\d+")))
  "regex/compile valid pattern")

(assert-err (fn () (compile-fn "[invalid"))
  "regex/compile invalid pattern")

(assert-err (fn () (compile-fn 42))
  "regex/compile wrong type")

(assert-err (fn () (compile-fn))
  "regex/compile wrong arity: no args")

(assert-err (fn () (compile-fn "a" "b"))
  "regex/compile wrong arity: two args")

# ── regex/match? ───────────────────────────────────────────────────

(assert-true (match-fn (compile-fn "\\d+") "abc123")
  "regex/match? true")

(assert-false (match-fn (compile-fn "\\d+") "abc")
  "regex/match? false")

(assert-err (fn () (match-fn "not-a-regex" "abc"))
  "regex/match? wrong type")

# ── regex/find ─────────────────────────────────────────────────────

(assert-eq (get (find-fn (compile-fn "\\d+") "abc123def") :match)
  "123"
  "regex/find match value")

(let ((m (find-fn (compile-fn "\\d+") "abc123def")))
  (assert-eq (get m :start) 3 "regex/find start")
  (assert-eq (get m :end) 6 "regex/find end"))

(assert-eq (find-fn (compile-fn "\\d+") "abc")
  nil
  "regex/find no match returns nil")

(assert-err (fn () (find-fn (compile-fn "x")))
  "regex/find wrong arity")

# ── regex/find-all ─────────────────────────────────────────────────

(assert-eq (length (find-all-fn (compile-fn "\\d+") "a1b22c333"))
  3
  "regex/find-all multiple matches count")

(assert-eq (get (first (find-all-fn (compile-fn "\\d+") "a1b22c333")) :match)
  "1"
  "regex/find-all first match value")

(assert-true (empty? (find-all-fn (compile-fn "\\d+") "abc"))
  "regex/find-all no matches")

# ── regex/captures ─────────────────────────────────────────────────

(let ((c (captures-fn (compile-fn "(\\d+)-(\\w+)") "42-hello")))
  (assert-eq (get c :0) "42-hello" "regex/captures group 0: full match")
  (assert-eq (get c :1) "42" "regex/captures group 1")
  (assert-eq (get c :2) "hello" "regex/captures group 2"))

(let ((c (captures-fn
            (compile-fn "(?P<year>\\d{4})-(?P<month>\\d{2})")
            "2024-01-15")))
  (assert-eq (get c :year) "2024" "regex/captures named: year")
  (assert-eq (get c :month) "01" "regex/captures named: month"))

(assert-eq (captures-fn (compile-fn "\\d+") "abc")
  nil
  "regex/captures no match returns nil")

(assert-err (fn () (captures-fn (compile-fn "x")))
  "regex/captures wrong arity")
