(import-file "tests/elle/assert.lisp")

## Regex plugin integration tests
## Tests the regex plugin (.so loaded via import-file)
## Migrated from tests/integration/regex.rs

## Try to load the regex plugin. If it fails, exit cleanly.
(let (([ok? _] (protect (import-file "target/debug/libelle_regex.so"))))
  (when (not ok?)
    (display "SKIP: regex plugin not built\n")
    (exit 0)))

# ── regex/compile ──────────────────────────────────────────────────

(assert-true
  (not (nil? (regex/compile "\\d+")))
  "regex/compile valid pattern")

(assert-err (fn () (regex/compile "[invalid"))
  "regex/compile invalid pattern")

(assert-err (fn () (regex/compile 42))
  "regex/compile wrong type")

(assert-err (fn () (regex/compile))
  "regex/compile wrong arity: no args")

(assert-err (fn () (regex/compile "a" "b"))
  "regex/compile wrong arity: two args")

# ── regex/match? ───────────────────────────────────────────────────

(assert-true (regex/match? (regex/compile "\\d+") "abc123")
  "regex/match? true")

(assert-false (regex/match? (regex/compile "\\d+") "abc")
  "regex/match? false")

(assert-err (fn () (regex/match? "not-a-regex" "abc"))
  "regex/match? wrong type")

# ── regex/find ─────────────────────────────────────────────────────

(assert-eq (get (regex/find (regex/compile "\\d+") "abc123def") :match)
  "123"
  "regex/find match value")

(let ((m (regex/find (regex/compile "\\d+") "abc123def")))
  (assert-eq (get m :start) 3 "regex/find start")
  (assert-eq (get m :end) 6 "regex/find end"))

(assert-eq (regex/find (regex/compile "\\d+") "abc")
  nil
  "regex/find no match returns nil")

(assert-err (fn () (regex/find (regex/compile "x")))
  "regex/find wrong arity")

# ── regex/find-all ─────────────────────────────────────────────────

(assert-eq (length (regex/find-all (regex/compile "\\d+") "a1b22c333"))
  3
  "regex/find-all multiple matches count")

(assert-eq (get (first (regex/find-all (regex/compile "\\d+") "a1b22c333")) :match)
  "1"
  "regex/find-all first match value")

(assert-true (empty? (regex/find-all (regex/compile "\\d+") "abc"))
  "regex/find-all no matches")

# ── regex/captures ─────────────────────────────────────────────────

(let ((c (regex/captures (regex/compile "(\\d+)-(\\w+)") "42-hello")))
  (assert-eq (get c :0) "42-hello" "regex/captures group 0: full match")
  (assert-eq (get c :1) "42" "regex/captures group 1")
  (assert-eq (get c :2) "hello" "regex/captures group 2"))

(let ((c (regex/captures
            (regex/compile "(?P<year>\\d{4})-(?P<month>\\d{2})")
            "2024-01-15")))
  (assert-eq (get c :year) "2024" "regex/captures named: year")
  (assert-eq (get c :month) "01" "regex/captures named: month"))

(assert-eq (regex/captures (regex/compile "\\d+") "abc")
  nil
  "regex/captures no match returns nil")

(assert-err (fn () (regex/captures (regex/compile "x")))
  "regex/captures wrong arity")
