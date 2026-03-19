(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## Compress plugin integration tests
## Tests the compress plugin (.so loaded via import-file)

## Try to load the compress plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_compress.so")))
(when (not ok?)
  (display "SKIP: compress plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def gzip-fn    (get plugin :gzip))
(def gunzip-fn  (get plugin :gunzip))
(def deflate-fn (get plugin :deflate))
(def inflate-fn (get plugin :inflate))
(def zstd-fn    (get plugin :zstd))
(def unzstd-fn  (get plugin :unzstd))

## ── gzip roundtrip ──────────────────────────────────────────────

(assert-string-eq
  (string (gunzip-fn (gzip-fn "hello")))
  "hello"
  "gzip roundtrip")

(assert-string-eq
  (string (gunzip-fn (gzip-fn "")))
  ""
  "gzip empty roundtrip")

## ── gzip with custom levels ─────────────────────────────────────

(assert-string-eq
  (string (gunzip-fn (gzip-fn "hello" 1)))
  "hello"
  "gzip level 1 roundtrip")

(assert-string-eq
  (string (gunzip-fn (gzip-fn "hello" 9)))
  "hello"
  "gzip level 9 roundtrip")

(assert-string-eq
  (string (gunzip-fn (gzip-fn "hello" 0)))
  "hello"
  "gzip level 0 roundtrip")

## ── deflate roundtrip ───────────────────────────────────────────

(assert-string-eq
  (string (inflate-fn (deflate-fn "hello")))
  "hello"
  "deflate roundtrip")

(assert-string-eq
  (string (inflate-fn (deflate-fn "")))
  ""
  "deflate empty roundtrip")

## ── zstd roundtrip ──────────────────────────────────────────────

(assert-string-eq
  (string (unzstd-fn (zstd-fn "hello")))
  "hello"
  "zstd roundtrip")

(assert-string-eq
  (string (unzstd-fn (zstd-fn "")))
  ""
  "zstd empty roundtrip")

(assert-string-eq
  (string (unzstd-fn (zstd-fn "hello" 1)))
  "hello"
  "zstd level 1 roundtrip")

(assert-string-eq
  (string (unzstd-fn (zstd-fn "hello" 22)))
  "hello"
  "zstd level 22 roundtrip")

## ── bytes and @string input ─────────────────────────────────────

(assert-string-eq
  (string (gunzip-fn (gzip-fn (bytes "hello"))))
  "hello"
  "gzip bytes input")

(assert-string-eq
  (string (gunzip-fn (gzip-fn @"hello")))
  "hello"
  "gzip @string input")

## ── bad data to decompress → compress-error ─────────────────────

(assert-err-kind
  (fn () (gunzip-fn "not gzip data"))
  :compress-error
  "gunzip bad data")

(assert-err-kind
  (fn () (inflate-fn "not deflate data"))
  :compress-error
  "inflate bad data")

(assert-err-kind
  (fn () (unzstd-fn "not zstd data"))
  :compress-error
  "unzstd bad data")

## ── bad level → compress-error ──────────────────────────────────

(assert-err
  (fn () (gzip-fn "hello" 99))
  "gzip bad level")

(assert-err
  (fn () (deflate-fn "hello" 10))
  "deflate level out of range")

(assert-err
  (fn () (zstd-fn "hello" 0))
  "zstd level 0 out of range")

(assert-err
  (fn () (zstd-fn "hello" 23))
  "zstd level 23 out of range")

## ── level wrong type → type-error ───────────────────────────────

(assert-err
  (fn () (gzip-fn "hello" "fast"))
  "gzip level wrong type")

(assert-err
  (fn () (zstd-fn "hello" "best"))
  "zstd level wrong type")

## ── wrong input type → type-error ───────────────────────────────

(assert-err
  (fn () (gzip-fn 42))
  "gzip wrong type")

(assert-err
  (fn () (gunzip-fn 42))
  "gunzip wrong type")

(assert-err
  (fn () (deflate-fn 42))
  "deflate wrong type")

(assert-err
  (fn () (inflate-fn 42))
  "inflate wrong type")

(assert-err
  (fn () (zstd-fn 42))
  "zstd wrong type")

(assert-err
  (fn () (unzstd-fn 42))
  "unzstd wrong type")
