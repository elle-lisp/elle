
## Compress plugin integration tests
## Tests the compress plugin (.so loaded via import-file)

## Try to load the compress plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_compress.so")))
(when (not ok?)
  (print "SKIP: compress plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def gzip-fn    (get plugin :gzip))
(def gunzip-fn  (get plugin :gunzip))
(def deflate-fn (get plugin :deflate))
(def inflate-fn (get plugin :inflate))
(def zstd-fn    (get plugin :zstd))
(def unzstd-fn  (get plugin :unzstd))

## ── gzip roundtrip ──────────────────────────────────────────────

(assert (= (string (gunzip-fn (gzip-fn "hello"))) "hello") "gzip roundtrip")

(assert (= (string (gunzip-fn (gzip-fn ""))) "") "gzip empty roundtrip")

## ── gzip with custom levels ─────────────────────────────────────

(assert (= (string (gunzip-fn (gzip-fn "hello" 1))) "hello") "gzip level 1 roundtrip")

(assert (= (string (gunzip-fn (gzip-fn "hello" 9))) "hello") "gzip level 9 roundtrip")

(assert (= (string (gunzip-fn (gzip-fn "hello" 0))) "hello") "gzip level 0 roundtrip")

## ── deflate roundtrip ───────────────────────────────────────────

(assert (= (string (inflate-fn (deflate-fn "hello"))) "hello") "deflate roundtrip")

(assert (= (string (inflate-fn (deflate-fn ""))) "") "deflate empty roundtrip")

## ── zstd roundtrip ──────────────────────────────────────────────

(assert (= (string (unzstd-fn (zstd-fn "hello"))) "hello") "zstd roundtrip")

(assert (= (string (unzstd-fn (zstd-fn ""))) "") "zstd empty roundtrip")

(assert (= (string (unzstd-fn (zstd-fn "hello" 1))) "hello") "zstd level 1 roundtrip")

(assert (= (string (unzstd-fn (zstd-fn "hello" 22))) "hello") "zstd level 22 roundtrip")

## ── bytes and @string input ─────────────────────────────────────

(assert (= (string (gunzip-fn (gzip-fn (bytes "hello")))) "hello") "gzip bytes input")

(assert (= (string (gunzip-fn (gzip-fn @"hello"))) "hello") "gzip @string input")

## ── bad data to decompress → compress-error ─────────────────────

(let (([ok? err] (protect ((fn () (gunzip-fn "not gzip data")))))) (assert (not ok?) "gunzip bad data") (assert (= (get err :error) :compress-error) "gunzip bad data"))

(let (([ok? err] (protect ((fn () (inflate-fn "not deflate data")))))) (assert (not ok?) "inflate bad data") (assert (= (get err :error) :compress-error) "inflate bad data"))

(let (([ok? err] (protect ((fn () (unzstd-fn "not zstd data")))))) (assert (not ok?) "unzstd bad data") (assert (= (get err :error) :compress-error) "unzstd bad data"))

## ── bad level → compress-error ──────────────────────────────────

(let (([ok? _] (protect ((fn () (gzip-fn "hello" 99)))))) (assert (not ok?) "gzip bad level"))

(let (([ok? _] (protect ((fn () (deflate-fn "hello" 10)))))) (assert (not ok?) "deflate level out of range"))

(let (([ok? _] (protect ((fn () (zstd-fn "hello" 0)))))) (assert (not ok?) "zstd level 0 out of range"))

(let (([ok? _] (protect ((fn () (zstd-fn "hello" 23)))))) (assert (not ok?) "zstd level 23 out of range"))

## ── level wrong type → type-error ───────────────────────────────

(let (([ok? _] (protect ((fn () (gzip-fn "hello" "fast")))))) (assert (not ok?) "gzip level wrong type"))

(let (([ok? _] (protect ((fn () (zstd-fn "hello" "best")))))) (assert (not ok?) "zstd level wrong type"))

## ── wrong input type → type-error ───────────────────────────────

(let (([ok? _] (protect ((fn () (gzip-fn 42)))))) (assert (not ok?) "gzip wrong type"))

(let (([ok? _] (protect ((fn () (gunzip-fn 42)))))) (assert (not ok?) "gunzip wrong type"))

(let (([ok? _] (protect ((fn () (deflate-fn 42)))))) (assert (not ok?) "deflate wrong type"))

(let (([ok? _] (protect ((fn () (inflate-fn 42)))))) (assert (not ok?) "inflate wrong type"))

(let (([ok? _] (protect ((fn () (zstd-fn 42)))))) (assert (not ok?) "zstd wrong type"))

(let (([ok? _] (protect ((fn () (unzstd-fn 42)))))) (assert (not ok?) "unzstd wrong type"))
