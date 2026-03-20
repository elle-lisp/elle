(elle/epoch 1)

## Glob plugin integration tests
## Tests the glob plugin (.so loaded via import-file)
##
## Plugin symbols (glob/glob, glob/match?, glob/match-path?) are only available
## at runtime after import-file loads the .so. Because file-as-letrec compiles
## the entire file before executing any of it, we use the struct returned by
## import-file to access plugin functions.

## Try to load the glob plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_glob.so")))
(when (not ok?)
  (display "SKIP: glob plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def glob-fn     (get plugin :glob))
(def match-fn    (get plugin :match?))
(def match-path-fn (get plugin :match-path?))

## ── glob/glob ──────────────────────────────────────────────────

(assert (array? (glob-fn "Cargo.toml")) "glob/glob returns array")

(let [(r1 (glob-fn "Cargo.toml"))]
  (assert (= (length r1) 1) "glob/glob finds cargo toml length")
  (assert (= (get r1 0) "Cargo.toml") "glob/glob finds cargo toml value"))

(assert (> (length (glob-fn "plugins/*/Cargo.toml")) 0) "glob/glob wildcard")

(assert (= (length (glob-fn "nonexistent_*.xyz")) 0) "glob/glob no matches")

(let (([ok? _] (protect ((fn () (glob-fn "[invalid")))))) (assert (not ok?) "glob/glob invalid pattern"))
(let (([ok? _] (protect ((fn () (glob-fn 42)))))) (assert (not ok?) "glob/glob wrong type"))

## ── glob/match? ────────────────────────────────────────────────

(assert (match-fn "*.rs" "main.rs") "glob/match true")
(assert (not (match-fn "*.rs" "main.py")) "glob/match false")
(let (([ok? _] (protect ((fn () (match-fn "[invalid" "test")))))) (assert (not ok?) "glob/match invalid pattern"))
(let (([ok? _] (protect ((fn () (match-fn 42 "test")))))) (assert (not ok?) "glob/match wrong type"))

## ── glob/match-path? ───────────────────────────────────────────

(assert (match-path-fn "src/*.rs" "src/main.rs") "glob/match-path true")
(assert (not (match-path-fn "*.py" "src/main.rs")) "glob/match-path false")
(let (([ok? _] (protect ((fn () (match-path-fn "[invalid" "test")))))) (assert (not ok?) "glob/match-path invalid pattern"))
