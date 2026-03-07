(import-file "tests/elle/assert.lisp")

## Glob plugin integration tests
## Tests the glob plugin (.so loaded via import-file)

## Try to load the glob plugin. If it fails, the file exits cleanly.
(protect
  (import-file "target/debug/libelle_glob.so")
  
  ## ── glob/glob ──────────────────────────────────────────────────
  
  (assert-true (array? (glob/glob "Cargo.toml")) "glob/glob returns array")
  
  (let [(r1 (glob/glob "Cargo.toml"))]
    (assert-eq (length r1) 1 "glob/glob finds cargo toml length")
    (assert-eq (get r1 0) "Cargo.toml" "glob/glob finds cargo toml value"))
  
  (assert-true (> (length (glob/glob "plugins/*/Cargo.toml")) 0) "glob/glob wildcard")
  
  (assert-eq (length (glob/glob "nonexistent_*.xyz")) 0 "glob/glob no matches")
  
  (assert-err (fn () (glob/glob "[invalid")) "glob/glob invalid pattern")
  (assert-err (fn () (glob/glob 42)) "glob/glob wrong type")
  
  ## ── glob/match? ────────────────────────────────────────────────
  
  (assert-true (glob/match? "*.rs" "main.rs") "glob/match true")
  (assert-false (glob/match? "*.rs" "main.py") "glob/match false")
  (assert-err (fn () (glob/match? "[invalid" "test")) "glob/match invalid pattern")
  (assert-err (fn () (glob/match? 42 "test")) "glob/match wrong type")
  
  ## ── glob/match-path? ───────────────────────────────────────────
  
  (assert-true (glob/match-path? "src/*.rs" "src/main.rs") "glob/match-path true")
  (assert-false (glob/match-path? "*.py" "src/main.rs") "glob/match-path false")
  (assert-err (fn () (glob/match-path? "[invalid" "test")) "glob/match-path invalid pattern")
  
  (error "plugin not available"))
