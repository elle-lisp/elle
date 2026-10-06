(elle/epoch 14)
# audited: 2026-10-06
# The HPACK Huffman decoder reads every code at every bit offset, takes EOS padding, and refuses EOS.
# lib/http2/overview.md
#
# Codes run from 5 to 30 bits, so a decoder that reads input a byte at a
# time meets a code starting at every one of the eight bit offsets, and a
# long code that spans up to five bytes. Each case below puts every byte
# value behind a prefix of 0 to 7 `a`s, whose 5-bit codes shift where the
# rest begins.
#
# The trap is the boundary between a code that fits a byte-wide lookup and
# one that does not: a decoder that tells the two apart by the next eight
# bits must still read a short code that ends within those bits correctly,
# and fall back to the bit walk for a long one, at every offset.

(def huffman ((import "std/http2/huffman")))

(def every-byte (apply bytes (range 0 256)))

(defn compression-error? [result]
  "True when a `protect` result is the decoder's compression error."
  (let [[ok? err] result]
    (and (not ok?) (= :h2-error (get err :error))
         (= :compression-error (get err :reason)))))

# ── Every byte, at every bit offset ──────────────────────────────────

(println "every byte value decodes at every bit offset...")

(each k in (range 0 8)
  (let [input (concat (bytes (string/repeat "a" k)) every-byte)]
    (assert (= input (huffman:decode (huffman:encode input)))
            (string "every byte value after " k " a's must decode back"))))

# ── Padding ──────────────────────────────────────────────────────────

(println "EOS padding ends the input...")

# One 5-bit code and three padding ones make a single byte.
(assert (= (bytes "a") (huffman:decode (bytes 0x1f)))
        "a code padded with ones to a byte must decode alone")
# Two 5-bit codes and six padding ones make two bytes.
(assert (= (bytes "aa") (huffman:decode (huffman:encode (bytes "aa"))))
        "two codes padded across a byte boundary must decode")
(assert (= (bytes) (huffman:decode (bytes))) "no input decodes to no bytes")

# ── EOS in the data ──────────────────────────────────────────────────

(println "an EOS code in the data is refused...")

# EOS is thirty ones; thirty-two ones hold it whole.
(assert (compression-error? (protect (huffman:decode (bytes 0xff 0xff 0xff 0xff))))
        "a whole EOS code must raise a compression error")
(assert (compression-error? (protect (huffman:decode (concat (huffman:encode (bytes "a"))
                                     (bytes 0xff 0xff 0xff 0xff)))))
        "an EOS code after a padded byte must raise a compression error")

(println "h2-huffman: every code decodes, and EOS is refused")
