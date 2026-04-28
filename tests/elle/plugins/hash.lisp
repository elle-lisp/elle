(elle/epoch 9)

## Hash plugin integration tests

(def [ok? hash] (protect (import-file "target/release/libelle_hash.so")))
(when (not ok?)
  (print "SKIP: hash plugin not built\n")
  (exit 0))

## ── MD5 ──────────────────────────────────────────────────────────

(assert (= (bytes->hex (hash:md5 "")) "d41d8cd98f00b204e9800998ecf8427e")
        "md5 empty")

(assert (= (bytes->hex (hash:md5 "hello")) "5d41402abc4b2a76b9719d911017c592")
        "md5 hello")

## ── SHA-1 ────────────────────────────────────────────────────────

(assert (= (bytes->hex (hash:sha1 ""))
           "da39a3ee5e6b4b0d3255bfef95601890afd80709")
        "sha1 empty")

(assert (= (bytes->hex (hash:sha1 "hello"))
           "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        "sha1 hello")

## ── SHA-256 ──────────────────────────────────────────────────────

(assert (= (bytes->hex (hash:sha256 ""))
           "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        "sha256 empty")

(assert (= (bytes->hex (hash:sha256 "hello"))
           "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        "sha256 hello")

## ── SHA-512 ──────────────────────────────────────────────────────

(assert (= (bytes->hex (hash:sha512 "hello"))
           "9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043")
        "sha512 hello")

## ── SHA3-256 ─────────────────────────────────────────────────────

(assert (= (bytes->hex (hash:sha3-256 ""))
           "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a")
        "sha3-256 empty")

(assert (= (bytes->hex (hash:sha3-256 "hello"))
           "3338be694f50c5f338814986cdf0686453a888b84f424d792af4b9202398f392")
        "sha3-256 hello")

## ── BLAKE2b-512 ──────────────────────────────────────────────────

(assert (= (bytes->hex (hash:blake2b-512 "hello"))
           "e4cfa39a3d37be31c59609e807970799caa68a19bfaa15135f165085e01d41a65ba1e1b146aeb6bd0092b49eac214c103ccfa3a365954bbbe52f74a2b3620c94")
        "blake2b-512 hello")

## ── BLAKE3 ───────────────────────────────────────────────────────

(assert (= (bytes->hex (hash:blake3 "hello"))
           "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f")
        "blake3 hello")

## ── BLAKE3 keyed ─────────────────────────────────────────────────

## key must be exactly 32 bytes; use blake3 output as the key
(let* [key (hash:blake3 "mykey")]
  (assert (= (length (hash:blake3-keyed key "hello")) 32)
          "blake3-keyed returns 32 bytes"))

## ── BLAKE3 derive ────────────────────────────────────────────────

(assert (= (length (hash:blake3-derive "myapp 2026" "secret")) 32)
        "blake3-derive returns 32 bytes")

## ── CRC32 ────────────────────────────────────────────────────────

(assert (= (hash:crc32 "") 0) "crc32 empty")
(assert (int? (hash:crc32 "hello")) "crc32 returns integer")

## ── xxHash ───────────────────────────────────────────────────────

(assert (int? (hash:xxh32 "hello")) "xxh32 returns integer")
(assert (int? (hash:xxh64 "hello")) "xxh64 returns integer")
(assert (= (length (hash:xxh128 "hello")) 16) "xxh128 returns 16 bytes")

## ── bytes input ──────────────────────────────────────────────────

(assert (= (hash:sha256 (bytes 104 101 108 108 111)) (hash:sha256 "hello"))
        "bytes input matches string input")

## ── Streaming API ────────────────────────────────────────────────

## new/update/finalize matches one-shot for all algorithms
(each algo in [:md5
               :sha1
               :sha256
               :sha512
               :sha3-256
               :blake2b-512
               :blake3
               :crc32
               :xxh128]
  (let* [ctx (hash:new algo)]
    (hash:update ctx "hel")
    (hash:update ctx "lo")
    (assert (= (hash:finalize ctx) ((get hash algo) "hello"))
            (concat "streaming matches one-shot for " (string algo)))))

## finalize resets — context is reusable
(let* [ctx (hash:new :sha256)]
  (hash:update ctx "hello")
  (def d1 (hash:finalize ctx))
  (hash:update ctx "hello")
  (assert (= d1 (hash:finalize ctx)) "finalize resets for reuse"))

## update returns the context (for stream/fold chaining)
(let* [ctx (hash:new :sha256)
       ret (hash:update ctx "hello")]
  (assert (= (hash:finalize ret) (hash:sha256 "hello"))
          "update returns the context"))

## stream/fold integration
(let* [chunks (coro/new (fn []
                          (yield "hel")
                          (yield "lo")))
       ctx (stream/fold hash:update (hash:new :sha256) chunks)]
  (assert (= (hash:finalize ctx) (hash:sha256 "hello"))
          "stream/fold with hash/update"))

## ── hash/hex ─────────────────────────────────────────────────────

(assert (= (hash:hex :sha256 "hello")
           "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        "hex sha256 hello")

(assert (= (hash:hex :md5 "") "d41d8cd98f00b204e9800998ecf8427e")
        "hex md5 empty")

## crc32 hex is an integer formatted as hex
(assert (string? (hash:hex :crc32 "hello")) "hex crc32 returns string")

## ── hash/algorithms ──────────────────────────────────────────────

(def algos (hash:algorithms))
(assert (set? algos) "algorithms returns a set")
(assert (has? algos :sha256) "algorithms contains :sha256")
(assert (has? algos :blake3) "algorithms contains :blake3")
(assert (has? algos :crc32) "algorithms contains :crc32")
(assert (has? algos :xxh64) "algorithms contains :xxh64")
(assert (not (has? algos :bogus)) "algorithms does not contain :bogus")

## ── error cases ──────────────────────────────────────────────────

(let [[ok? _] (protect (hash:new :bogus))]
  (assert (not ok?) "unknown algorithm errors"))

(let [[ok? _] (protect (hash:update "not-a-context" "data"))]
  (assert (not ok?) "update on non-context errors"))

(print "hash: all tests passed\n")
