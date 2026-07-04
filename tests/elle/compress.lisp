(elle/epoch 12)
## Compress module tests (FFI to libz + libzstd)

# Gate the whole file on libz + libzstd: if either can't load, re-raise as a loud
# :gated so `elle test` records a file-level SKIP with a reason (docs § Gating).
# Eager (def …), so it gates during barrier-module setup, before any test thunk.
# Never (exit 0): under the runner that would kill the process mid-run.
(def _compress-libs
  (let [z (protect (ffi/native "libz.so"))
        zstd (protect (ffi/native "libzstd.so"))]
    (if (and (get z 0) (get zstd 0))
      true
      (error (struct :error :gated :reason "libz.so or libzstd.so not installed")))))

(def z ((import "std/compress")))

## gzip roundtrip
(let* [c (z:gzip "hello world")
       d (z:gunzip c)]
  (assert (= d (bytes "hello world")) "gzip roundtrip"))

## gzip with custom level
(let* [c (z:gzip "hello world" 1)
       d (z:gunzip c)]
  (assert (= d (bytes "hello world")) "gzip level 1"))

## zlib roundtrip
(let* [c (z:zlib "hello world")
       d (z:unzlib c)]
  (assert (= d (bytes "hello world")) "zlib roundtrip"))

## raw deflate roundtrip
(let* [c (z:deflate "hello world")
       d (z:inflate c)]
  (assert (= d (bytes "hello world")) "deflate roundtrip"))

## zstd roundtrip
(let* [c (z:zstd "hello world")
       d (z:unzstd c)]
  (assert (= d (bytes "hello world")) "zstd roundtrip"))

## zstd with custom level
(let* [c (z:zstd "hello world" 1)
       d (z:unzstd c)]
  (assert (= d (bytes "hello world")) "zstd level 1"))

## bytes input
(assert (= (z:gunzip (z:gzip (bytes "test"))) (bytes "test")) "bytes input gzip")
(assert (= (z:unzstd (z:zstd (bytes "test"))) (bytes "test")) "bytes input zstd")

## compression reduces size on compressible data
(let [big (string/join (map (fn [_] "hello ") (->list (range 100))) "")]
  (assert (< (length (z:gzip big)) (length (bytes big))) "gzip compresses")
  (assert (< (length (z:zstd big)) (length (bytes big))) "zstd compresses"))

## empty input
(assert (= (z:gunzip (z:gzip "")) (bytes "")) "gzip empty")
(assert (= (z:unzlib (z:zlib "")) (bytes "")) "zlib empty")
(assert (= (z:inflate (z:deflate "")) (bytes "")) "deflate empty")
(assert (= (z:unzstd (z:zstd "")) (bytes "")) "zstd empty")

(println "compress: all tests passed")
