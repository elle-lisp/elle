(elle/epoch 9)
## Compress module tests (FFI to libz + libzstd)

(def [ok1? _] (protect ((fn [] (ffi/native "libz.so")))))
(def [ok2? _] (protect ((fn [] (ffi/native "libzstd.so")))))
(unless (and ok1? ok2?)
  (println "SKIP: libz.so or libzstd.so not available")
  (exit 0))

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
