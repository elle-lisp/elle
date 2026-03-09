(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## Glob plugin integration tests
## Tests the glob plugin (.so loaded via import-file)
##
## Plugin symbols (glob/glob, glob/match?, glob/match-path?) are only available
## at runtime after import-file loads the .so. Because file-as-letrec compiles
## the entire file before executing any of it, we use the struct returned by
## import-file to access plugin functions.

## Try to load the glob plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/debug/libelle_glob.so")))
(when (not ok?)
  (display "SKIP: glob plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def glob-fn     (get plugin :glob))
(def match-fn    (get plugin :match?))
(def match-path-fn (get plugin :match-path?))

## ── glob/glob ──────────────────────────────────────────────────

(assert-true (array? (glob-fn "Cargo.toml")) "glob/glob returns array")

(let [(r1 (glob-fn "Cargo.toml"))]
  (assert-eq (length r1) 1 "glob/glob finds cargo toml length")
  (assert-eq (get r1 0) "Cargo.toml" "glob/glob finds cargo toml value"))

(assert-true (> (length (glob-fn "plugins/*/Cargo.toml")) 0) "glob/glob wildcard")

(assert-eq (length (glob-fn "nonexistent_*.xyz")) 0 "glob/glob no matches")

(assert-err (fn () (glob-fn "[invalid")) "glob/glob invalid pattern")
(assert-err (fn () (glob-fn 42)) "glob/glob wrong type")

## ── glob/match? ────────────────────────────────────────────────

(assert-true (match-fn "*.rs" "main.rs") "glob/match true")
(assert-false (match-fn "*.rs" "main.py") "glob/match false")
(assert-err (fn () (match-fn "[invalid" "test")) "glob/match invalid pattern")
(assert-err (fn () (match-fn 42 "test")) "glob/match wrong type")

## ── glob/match-path? ───────────────────────────────────────────

(assert-true (match-path-fn "src/*.rs" "src/main.rs") "glob/match-path true")
(assert-false (match-path-fn "*.py" "src/main.rs") "glob/match-path false")
(assert-err (fn () (match-path-fn "[invalid" "test")) "glob/match-path invalid pattern")
