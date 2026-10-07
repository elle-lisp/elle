(elle/epoch 14)
# audited: 2026-10-06
# The HPACK encoder Huffman-codes a string only when the code is shorter than the string.
# lib/http2/overview.md
#
# Every case encodes one `user-agent` header with a fresh encoder. The
# name sits in the static table at index 58, so the block opens with the
# single byte 0x7a (literal with incremental indexing, indexed name) and
# the value's string literal starts at byte 1. That byte's high bit is
# the Huffman flag, and its low seven bits are the literal's length.
#
# `X` and `Z` carry 8-bit Huffman codes, `a` a 5-bit one. A value of `X`
# and `Z` alone codes to exactly its own length, so the shorter of the
# two is a tie, and a tie goes raw.
#
# The counter-factual is an encoder that Huffman-codes every string. A
# value that does not shrink then costs the peer a bit-by-bit decode for
# no saving, and a header-heavy exchange pays that on every byte.

(def huffman ((import "std/http2/huffman")))
(def hpack ((import "std/http2/hpack") :huffman huffman))

(defn value-literal [value]
  "The first byte of `value`'s string literal, and the block, when one
   fresh encoder encodes a user-agent header carrying it."
  (let [block (hpack:encode (hpack:make-encoder) [["user-agent" value]])]
    (assert (= 0x7a (get block 0))
            "user-agent must encode as an indexed-name literal")
    [(get block 1) block]))

(defn huffman-flag? [b]
  "True when a string literal's first byte carries the Huffman flag."
  (not (= 0 (bit/and b 0x80))))

(defn round-trip [block]
  "The value one fresh decoder reads back out of `block`."
  (get (get (hpack:decode (hpack:make-decoder) block) 0) 1))

# ── A string the code does not shorten goes raw ──────────────────────

(println "a string whose code is no shorter goes raw...")

(let [[first-byte block] (value-literal "XZXZXZXZ")]
  (assert (not (huffman-flag? first-byte))
          "eight 8-bit codes make eight bytes, so the literal must go raw")
  (assert (= 8 (bit/and first-byte 0x7f)) "a raw literal carries its own length")
  (assert (= "XZXZXZXZ" (round-trip block)) "a raw literal must decode back"))

(let [[first-byte block] (value-literal "XZXZXZXZa")]
  (assert (not (huffman-flag? first-byte))
          "69 bits pad to nine bytes, a tie with the string, so it must go raw")
  (assert (= "XZXZXZXZa" (round-trip block)) "the tie must decode back"))

# ── A string the code shortens is Huffman-coded ──────────────────────

(println "a string whose code is shorter is Huffman-coded...")

(let [[first-byte block] (value-literal "aaaaaaaa")]
  (assert (huffman-flag? first-byte)
          "eight 5-bit codes make five bytes, so the literal must be Huffman-coded")
  (assert (= 5 (bit/and first-byte 0x7f))
          "the coded literal carries the coded length")
  (assert (= "aaaaaaaa" (round-trip block)) "the coded literal must decode back"))

# A code longer than a byte, after short ones: the backslash takes 19 bits
# and crosses two byte boundaries, and the string still shrinks.
(let [value (string (string/repeat "a" 20) "\\")
      [first-byte block] (value-literal value)]
  (assert (huffman-flag? first-byte) "119 bits against 21 bytes must code")
  (assert (= value (round-trip block))
          "a code crossing two bytes must decode back"))

(println "h2-hpack-strings: the encoder sends the shorter of the two")
