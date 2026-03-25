(elle/epoch 6)

## Semver plugin integration tests
## Tests the semver plugin (.so loaded via import-file)

## Try to load the semver plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-native "semver")))
(when (not ok?)
  (print "SKIP: semver plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn     (get plugin :parse))
(def valid-fn     (get plugin :valid?))
(def compare-fn   (get plugin :compare))
(def satisfies-fn (get plugin :satisfies?))
(def increment-fn (get plugin :increment))

## ── semver/parse ────────────────────────────────────────────────

(def v (parse-fn "1.2.3"))
(assert (= (get v :major) 1) "semver/parse major")
(assert (= (get v :minor) 2) "semver/parse minor")
(assert (= (get v :patch) 3) "semver/parse patch")
(assert (= (get v :pre) "") "semver/parse pre empty")
(assert (= (get v :build) "") "semver/parse build empty")

## Parse with pre-release
(def v2 (parse-fn "1.0.0-alpha.1"))
(assert (= (get v2 :pre) "alpha.1") "semver/parse pre-release")

## Parse with build metadata
(def v3 (parse-fn "1.0.0+build.123"))
(assert (= (get v3 :build) "build.123") "semver/parse build metadata")

## ── semver/valid? ───────────────────────────────────────────────

(assert (valid-fn "1.2.3") "semver/valid? good")
(assert (not (valid-fn "not.a.version")) "semver/valid? bad")
(assert (not (valid-fn "")) "semver/valid? empty")

## ── semver/compare ──────────────────────────────────────────────

(assert (= (compare-fn "1.0.0" "2.0.0") -1) "semver/compare less")
(assert (= (compare-fn "1.0.0" "1.0.0") 0) "semver/compare equal")
(assert (= (compare-fn "2.0.0" "1.0.0") 1) "semver/compare greater")
(assert (= (compare-fn "1.0.0-alpha" "1.0.0") -1) "semver/compare pre < release")

## ── semver/satisfies? ───────────────────────────────────────────

(assert (satisfies-fn "1.2.3" ">=1.0.0") "semver/satisfies? ge")
(assert (not (satisfies-fn "0.9.0" ">=1.0.0")) "semver/satisfies? not ge")
(assert (satisfies-fn "1.2.3" ">=1.0.0, <2.0.0") "semver/satisfies? range")

## ── semver/increment ────────────────────────────────────────────

(assert (= (increment-fn "1.2.3" :patch) "1.2.4") "semver/increment patch")
(assert (= (increment-fn "1.2.3" :minor) "1.3.0") "semver/increment minor")
(assert (= (increment-fn "1.2.3" :major) "2.0.0") "semver/increment major")

## Increment clears pre-release
(assert (= (increment-fn "1.0.0-alpha" :patch) "1.0.1") "semver/increment clears pre")

## ── error cases ─────────────────────────────────────────────────

## Invalid version string → semver-error
(let (([ok? err] (protect ((fn () (parse-fn "nope")))))) (assert (not ok?) "semver/parse invalid") (assert (= (get err :error) :semver-error) "semver/parse invalid"))
(let (([ok? err] (protect ((fn () (compare-fn "nope" "1.0.0")))))) (assert (not ok?) "semver/compare invalid") (assert (= (get err :error) :semver-error) "semver/compare invalid"))

## Wrong type → type-error
(let (([ok? _] (protect ((fn () (parse-fn 42)))))) (assert (not ok?) "semver/parse wrong type"))

## String (not keyword) passed to increment → type-error
(let (([ok? _] (protect ((fn () (increment-fn "1.0.0" "patch")))))) (assert (not ok?) "semver/increment non-keyword"))
