(elle/epoch 6)

## UUID plugin integration tests

(def [ok? plugin] (protect (import-native "uuid")))
(when (not ok?)
  (print "SKIP: uuid plugin not built\n")
  (exit 0))

(def v4-fn      (get plugin :v4))
(def v5-fn      (get plugin :v5))
(def parse-fn   (get plugin :parse))
(def nil-fn     (get plugin :nil))
(def version-fn (get plugin :version))

## DNS namespace UUID (RFC 4122)
(def dns-ns "6ba7b810-9dad-11d1-80b4-00c04fd430c8")

# ── uuid/v4 ────────────────────────────────────────────────────────

## v4 returns a 36-character string (checked by round-tripping through parse)
(let ((u (v4-fn)))
  (assert (not (nil? (parse-fn u))) "uuid/v4 output is a valid UUID"))

## v4 generates distinct values (probabilistically; collision is astronomically unlikely)
(assert (not (= (v4-fn) (v4-fn))) "uuid/v4 generates distinct values")

# ── uuid/v5 ────────────────────────────────────────────────────────

## v5 with DNS namespace + "example.com" is a known deterministic value
(assert (= (v5-fn dns-ns "example.com") "cfbff0d1-9375-5685-968c-48ce8b15ae17") "uuid/v5 DNS/example.com known value")

## v5 is deterministic: same inputs -> same output
(assert (= (v5-fn dns-ns "hello") (v5-fn dns-ns "hello")) "uuid/v5 is deterministic")

## v5 with different names produces different UUIDs
(assert (not (= (v5-fn dns-ns "foo") (v5-fn dns-ns "bar"))) "uuid/v5 different names produce different UUIDs")

# ── uuid/parse ─────────────────────────────────────────────────────

## parse accepts uppercase and lowercases it
(assert (= (parse-fn "550E8400-E29B-41D4-A716-446655440000") "550e8400-e29b-41d4-a716-446655440000") "uuid/parse normalizes uppercase to lowercase")

## parse accepts lowercase unchanged
(assert (= (parse-fn "550e8400-e29b-41d4-a716-446655440000") "550e8400-e29b-41d4-a716-446655440000") "uuid/parse lowercase passthrough")

## parse rejects invalid strings
(let (([ok? err] (protect ((fn () (parse-fn "not-a-uuid")))))) (assert (not ok?) "uuid/parse rejects invalid UUID") (assert (= (get err :error) :uuid-error) "uuid/parse rejects invalid UUID"))

## parse rejects non-string
(let (([ok? err] (protect ((fn () (parse-fn 42)))))) (assert (not ok?) "uuid/parse rejects non-string") (assert (= (get err :error) :type-error) "uuid/parse rejects non-string"))

# ── uuid/nil ───────────────────────────────────────────────────────

(assert (= (nil-fn) "00000000-0000-0000-0000-000000000000") "uuid/nil returns all-zeros UUID")

# ── uuid/version ───────────────────────────────────────────────────

## v4 UUID has version 4
(assert (= (version-fn (v4-fn)) 4) "uuid/version on v4 returns 4")

## v5 UUID has version 5
(assert (= (version-fn (v5-fn dns-ns "test")) 5) "uuid/version on v5 returns 5")

## nil UUID version is nil (version 0)
(assert (= (version-fn (nil-fn)) nil) "uuid/version on nil UUID returns nil")

## version rejects non-string
(let (([ok? err] (protect ((fn () (version-fn 42)))))) (assert (not ok?) "uuid/version rejects non-string") (assert (= (get err :error) :type-error) "uuid/version rejects non-string"))

## version rejects invalid UUID
(let (([ok? err] (protect ((fn () (version-fn "bad")))))) (assert (not ok?) "uuid/version rejects invalid UUID") (assert (= (get err :error) :uuid-error) "uuid/version rejects invalid UUID"))
