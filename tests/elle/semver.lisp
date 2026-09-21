(elle/epoch 12)
# audited: 2026-09-21
# std/semver — the version arithmetic pinned against the semver 2.0.0
# spec. This corpus is the library's own arbitration seed: a release of
# std/semver is judged by these tests (docs/versioning.md).
#
# The counter-factual: pre-release identifiers order numerically when
# both sides are digits, so a lexical compare calls alpha.10 < alpha.2
# and nothing else in the tree would notice.

(def sv ((import "std/semver")))

# ── parse ──────────────────────────────────────────────────────────
(let [v (sv:parse "1.2.3")]
  (assert (= (v :major) 1) "major")
  (assert (= (v :minor) 2) "minor")
  (assert (= (v :patch) 3) "patch")
  (assert (= (v :pre) "") "no pre-release")
  (assert (= (v :build) "") "no build metadata"))

(let [v (sv:parse "1.2.3-alpha.1+build.5")]
  (assert (= (v :pre) "alpha.1") "pre-release rides after the dash")
  (assert (= (v :build) "build.5") "build rides after the plus"))

(let [v (sv:parse "1.0.0-x-y-z.-+meta-hyphen")]
  (assert (= (v :pre) "x-y-z.-") "hyphens inside pre survive")
  (assert (= (v :build) "meta-hyphen") "hyphens inside build survive"))

(defn parse-fails? [s]
  (not (first (protect (sv:parse s)))))

(assert (parse-fails? "1.2") "two components refuse")
(assert (parse-fails? "1.2.3.4") "four components refuse")
(assert (parse-fails? "01.2.3") "leading zero refuses")
(assert (parse-fails? "1.a.3") "non-numeric refuses")
(assert (parse-fails? "") "empty refuses")

(assert (sv:valid? "0.1.0") "valid version")
(assert (not (sv:valid? "1.2")) "invalid version")

# ── compare ────────────────────────────────────────────────────────
(assert (= (sv:compare "1.2.3" "1.2.3") 0) "equal")
(assert (= (sv:compare "1.2.3" "2.0.0") -1) "major orders first")
(assert (= (sv:compare "1.3.0" "1.2.9") 1) "minor beats patch")
(assert (= (sv:compare "1.2.3" "1.2.4") -1) "patch orders last")
(assert (= (sv:compare "1.0.0+a" "1.0.0+b") 0) "build metadata is ignored")

# The spec's §11 ordering chain, adjacent pairs.
(def chain
  ["1.0.0-alpha" "1.0.0-alpha.1" "1.0.0-alpha.beta" "1.0.0-beta" "1.0.0-beta.2"
   "1.0.0-beta.11" "1.0.0-rc.1" "1.0.0"])
(let [@i 0]
  (while (< i (- (length chain) 1))
    (assert (= (sv:compare (chain i) (chain (+ i 1))) -1)
            (string (chain i) " < " (chain (+ i 1))))
    (assign i (inc i))))
(assert (= (sv:compare "1.0.0-alpha.10" "1.0.0-alpha.2") 1)
        "numeric identifiers compare numerically")

# ── satisfies? ─────────────────────────────────────────────────────
(assert (sv:satisfies? "1.2.3" ">=1.0.0") "at least")
(assert (not (sv:satisfies? "0.9.0" ">=1.0.0")) "below at-least")
(assert (sv:satisfies? "1.2.3" ">1.2.2, <1.3.0") "a comma is an and")
(assert (not (sv:satisfies? "1.3.0" ">1.2.2, <1.3.0")) "outside the and")
(assert (sv:satisfies? "1.2.3" "=1.2.3") "exact")
(assert (sv:satisfies? "1.2.3" "!=1.2.4") "excluded")
(assert (sv:satisfies? "1.9.9" "^1.2.3") "caret spans the major")
(assert (not (sv:satisfies? "2.0.0" "^1.2.3")) "caret stops at the major")
(assert (sv:satisfies? "0.2.9" "^0.2.3") "pre-1.0 caret spans the minor")
(assert (not (sv:satisfies? "0.3.0" "^0.2.3")) "pre-1.0 caret stops there")
(assert (sv:satisfies? "1.2.9" "~1.2.3") "tilde spans the patch")
(assert (not (sv:satisfies? "1.3.0" "~1.2.3")) "tilde stops at the minor")
(assert (sv:satisfies? "1.2.3" "1.2.3") "a bare version reads as caret")

# ── increment ──────────────────────────────────────────────────────
(assert (= (sv:increment "1.2.3" :major) "2.0.0") "major zeroes the rest")
(assert (= (sv:increment "1.2.3" :minor) "1.3.0") "minor zeroes the patch")
(assert (= (sv:increment "1.2.3" :patch) "1.2.4") "patch increments")
(assert (= (sv:increment "1.2.3-alpha" :patch) "1.2.4") "increment clears pre")
(assert (not (first (protect (sv:increment "1.2.3" :epoch))))
        "an unknown part refuses")

(println "semver: all tests passed")
