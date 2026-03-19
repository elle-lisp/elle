(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## Semver plugin integration tests
## Tests the semver plugin (.so loaded via import-file)

## Try to load the semver plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_semver.so")))
(when (not ok?)
  (display "SKIP: semver plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn     (get plugin :parse))
(def valid-fn     (get plugin :valid?))
(def compare-fn   (get plugin :compare))
(def satisfies-fn (get plugin :satisfies?))
(def increment-fn (get plugin :increment))

## ── semver/parse ────────────────────────────────────────────────

(def v (parse-fn "1.2.3"))
(assert-eq (get v :major) 1 "semver/parse major")
(assert-eq (get v :minor) 2 "semver/parse minor")
(assert-eq (get v :patch) 3 "semver/parse patch")
(assert-eq (get v :pre) "" "semver/parse pre empty")
(assert-eq (get v :build) "" "semver/parse build empty")

## Parse with pre-release
(def v2 (parse-fn "1.0.0-alpha.1"))
(assert-eq (get v2 :pre) "alpha.1" "semver/parse pre-release")

## Parse with build metadata
(def v3 (parse-fn "1.0.0+build.123"))
(assert-eq (get v3 :build) "build.123" "semver/parse build metadata")

## ── semver/valid? ───────────────────────────────────────────────

(assert-true (valid-fn "1.2.3") "semver/valid? good")
(assert-false (valid-fn "not.a.version") "semver/valid? bad")
(assert-false (valid-fn "") "semver/valid? empty")

## ── semver/compare ──────────────────────────────────────────────

(assert-eq (compare-fn "1.0.0" "2.0.0") -1 "semver/compare less")
(assert-eq (compare-fn "1.0.0" "1.0.0") 0 "semver/compare equal")
(assert-eq (compare-fn "2.0.0" "1.0.0") 1 "semver/compare greater")
(assert-eq (compare-fn "1.0.0-alpha" "1.0.0") -1 "semver/compare pre < release")

## ── semver/satisfies? ───────────────────────────────────────────

(assert-true (satisfies-fn "1.2.3" ">=1.0.0") "semver/satisfies? ge")
(assert-false (satisfies-fn "0.9.0" ">=1.0.0") "semver/satisfies? not ge")
(assert-true (satisfies-fn "1.2.3" ">=1.0.0, <2.0.0") "semver/satisfies? range")

## ── semver/increment ────────────────────────────────────────────

(assert-string-eq (increment-fn "1.2.3" :patch) "1.2.4" "semver/increment patch")
(assert-string-eq (increment-fn "1.2.3" :minor) "1.3.0" "semver/increment minor")
(assert-string-eq (increment-fn "1.2.3" :major) "2.0.0" "semver/increment major")

## Increment clears pre-release
(assert-string-eq (increment-fn "1.0.0-alpha" :patch) "1.0.1"
  "semver/increment clears pre")

## ── error cases ─────────────────────────────────────────────────

## Invalid version string → semver-error
(assert-err-kind (fn () (parse-fn "nope")) :semver-error "semver/parse invalid")
(assert-err-kind (fn () (compare-fn "nope" "1.0.0")) :semver-error
  "semver/compare invalid")

## Wrong type → type-error
(assert-err (fn () (parse-fn 42)) "semver/parse wrong type")

## String (not keyword) passed to increment → type-error
(assert-err (fn () (increment-fn "1.0.0" "patch")) "semver/increment non-keyword")
