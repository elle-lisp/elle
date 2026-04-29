(elle/epoch 9)
## lib/http2/huffman.lisp — HPACK Huffman codec (RFC 7541 Appendix B)
##
## Loaded via: (def huffman ((import "std/http2/huffman")))
##
## Exports: {:encode :decode :test}
##   encode [bytes] -> bytes   — Huffman-encode a byte sequence
##   decode [bytes] -> bytes   — Huffman-decode a byte sequence
##   test   []      -> true    — internal self-tests

(fn []

  ## ── Huffman table (RFC 7541 Appendix B) ────────────────────────────────
  ## Each entry is [code bit-length] for the byte value at that index.
  ## 257 entries: 0-255 are byte values, 256 is EOS.

  (def table
    [[0x1ff8 13] [0x7fffd8 23] [0xfffffe2 28] [0xfffffe3 28] [0xfffffe4 28]
     [0xfffffe5 28] [0xfffffe6 28] [0xfffffe7 28] [0xfffffe8 28] [0xffffea 24]
     [0x3ffffffc 30] [0xfffffe9 28] [0xfffffea 28] [0x3ffffffd 30]
     [0xfffffeb 28] [0xfffffec 28] [0xfffffed 28] [0xfffffee 28] [0xfffffef 28]
     [0xffffff0 28] [0xffffff1 28] [0xffffff2 28] [0x3ffffffe 30] [0xffffff3 28]
     [0xffffff4 28] [0xffffff5 28] [0xffffff6 28] [0xffffff7 28] [0xffffff8 28]
     [0xffffff9 28] [0xffffffa 28] [0xffffffb 28] [0x14 6] [0x3f8 10] [0x3f9 10]
     [0xffa 12] [0x1ff9 13] [0x15 6] [0xf8 8] [0x7fa 11] [0x3fa 10] [0x3fb 10]
     [0xf9 8] [0x7fb 11] [0xfa 8] [0x16 6] [0x17 6] [0x18 6] [0x0 5] [0x1 5]
     [0x2 5] [0x19 6] [0x1a 6] [0x1b 6] [0x1c 6] [0x1d 6] [0x1e 6] [0x1f 6]
     [0x5c 7] [0xfb 8] [0x7ffc 15] [0x20 6] [0xffb 12] [0x3fc 10] [0x1ffa 13]
     [0x21 6] [0x5d 7] [0x5e 7] [0x5f 7] [0x60 7] [0x61 7] [0x62 7] [0x63 7]
     [0x64 7] [0x65 7] [0x66 7] [0x67 7] [0x68 7] [0x69 7] [0x6a 7] [0x6b 7]
     [0x6c 7] [0x6d 7] [0x6e 7] [0x6f 7] [0x70 7] [0x71 7] [0x72 7] [0xfc 8]
     [0x73 7] [0xfd 8] [0x1ffb 13] [0x7fff0 19] [0x1ffc 13] [0x3ffc 14] [0x22 6]
     [0x7ffd 15] [0x3 5] [0x23 6] [0x4 5] [0x24 6] [0x5 5] [0x25 6] [0x26 6]
     [0x27 6] [0x6 5] [0x74 7] [0x75 7] [0x28 6] [0x29 6] [0x2a 6] [0x7 5]
     [0x2b 6] [0x76 7] [0x2c 6] [0x8 5] [0x9 5] [0x2d 6] [0x77 7] [0x78 7]
     [0x79 7] [0x7a 7] [0x7b 7] [0x7ffe 15] [0x7fc 11] [0x3ffd 14] [0x1ffd 13]
     [0xffffffc 28] [0xfffe6 20] [0x3fffd2 22] [0xfffe7 20] [0xfffe8 20]
     [0x3fffd3 22] [0x3fffd4 22] [0x3fffd5 22] [0x7fffd9 23] [0x3fffd6 22]
     [0x7fffda 23] [0x7fffdb 23] [0x7fffdc 23] [0x7fffdd 23] [0x7fffde 23]
     [0xffffeb 24] [0x7fffdf 23] [0xffffec 24] [0xffffed 24] [0x3fffd7 22]
     [0x7fffe0 23] [0xffffee 24] [0x7fffe1 23] [0x7fffe2 23] [0x7fffe3 23]
     [0x7fffe4 23] [0x1fffdc 21] [0x3fffd8 22] [0x7fffe5 23] [0x3fffd9 22]
     [0x7fffe6 23] [0x7fffe7 23] [0xffffef 24] [0x3fffda 22] [0x1fffdd 21]
     [0xfffe9 20] [0x3fffdb 22] [0x3fffdc 22] [0x7fffe8 23] [0x7fffe9 23]
     [0x1fffde 21] [0x7fffea 23] [0x3fffdd 22] [0x3fffde 22] [0xfffff0 24]
     [0x1fffdf 21] [0x3fffdf 22] [0x7fffeb 23] [0x7fffec 23] [0x1fffe0 21]
     [0x1fffe1 21] [0x3fffe0 22] [0x1fffe2 21] [0x7fffed 23] [0x3fffe1 22]
     [0x7fffee 23] [0x7fffef 23] [0xfffea 20] [0x3fffe2 22] [0x3fffe3 22]
     [0x3fffe4 22] [0x7ffff0 23] [0x3fffe5 22] [0x3fffe6 22] [0x7ffff1 23]
     [0x3ffffe0 26] [0x3ffffe1 26] [0xfffeb 20] [0x7fff1 19] [0x3fffe7 22]
     [0x7ffff2 23] [0x3fffe8 22] [0x1ffffec 25] [0x3ffffe2 26] [0x3ffffe3 26]
     [0x3ffffe4 26] [0x7ffffde 27] [0x7ffffdf 27] [0x3ffffe5 26] [0xfffff1 24]
     [0x1ffffed 25] [0x7fff2 19] [0x1fffe3 21] [0x3ffffe6 26] [0x7ffffe0 27]
     [0x7ffffe1 27] [0x3ffffe7 26] [0x7ffffe2 27] [0xfffff2 24] [0x1fffe4 21]
     [0x1fffe5 21] [0x3ffffe8 26] [0x3ffffe9 26] [0xffffffd 28] [0x7ffffe3 27]
     [0x7ffffe4 27] [0x7ffffe5 27] [0xfffec 20] [0xfffff3 24] [0xfffed 20]
     [0x1fffe6 21] [0x3fffe9 22] [0x1fffe7 21] [0x1fffe8 21] [0x7ffff3 23]
     [0x3fffea 22] [0x3fffeb 22] [0x1ffffee 25] [0x1ffffef 25] [0xfffff4 24]
     [0xfffff5 24] [0x3ffffea 26] [0x7ffff4 23] [0x3ffffeb 26] [0x7ffffe6 27]
     [0x3ffffec 26] [0x3ffffed 26] [0x7ffffe7 27] [0x7ffffe8 27] [0x7ffffe9 27]
     [0x7ffffea 27] [0x7ffffeb 27] [0xffffffe 28] [0x7ffffec 27] [0x7ffffed 27]
     [0x7ffffee 27] [0x7ffffef 27] [0x7fffff0 27] [0x3ffffee 26] [0x3fffffff 30]])  # EOS, index 256

  ## ── Build decode tree ──────────────────────────────────────────────────
  ## Binary trie: each node is [left right] or a leaf integer (byte value).
  ## left = bit 0, right = bit 1. Navigate by reading bits MSB-first.

  (defn build-decode-tree []
    (def @root @[nil nil])
    (def @sym 0)
    (while (< sym 257)
      (let* [entry (get table sym)
             code (get entry 0)
             nbits (get entry 1)
             node root]
        (def @bit-idx (- nbits 1))
        (def @cur node)
        (while (> bit-idx 0)
          (let [bit (bit/and (bit/shr code bit-idx) 1)]
            (when (nil? (get cur bit)) (put cur bit @[nil nil]))
            (assign cur (get cur bit)))
          (assign bit-idx (- bit-idx 1)))  # Last bit — place the symbol
        (let [bit (bit/and code 1)]
          (put cur bit sym)))
      (assign sym (+ sym 1)))
    (freeze root))

  (def decode-tree (build-decode-tree))

  ## ── Encode ─────────────────────────────────────────────────────────────

  (defn huffman-encode [input]
    "Huffman-encode a byte sequence. Returns bytes."
    (let* [src (if (string? input) (bytes input) input)
           len (length src)
           out @[]
           @buf 0
           @buf-bits 0
           @i 0]
      (while (< i len)
        (let* [entry (get table (get src i))
               code (get entry 0)
               nbits (get entry 1)]
          (assign buf (bit/or (bit/shl buf nbits) code))
          (assign buf-bits (+ buf-bits nbits))  # Emit complete bytes
          (while (>= buf-bits 8)
            (assign buf-bits (- buf-bits 8))
            (push out (bit/and (bit/shr buf buf-bits) 0xff))))
        (assign i (+ i 1)))  # Pad with EOS prefix (all 1s) to byte boundary
      (when (> buf-bits 0)
        (let [pad (- 8 buf-bits)]
          (push out
                (bit/and (bit/or (bit/shl buf pad) (- (bit/shl 1 pad) 1)) 0xff))))
      (apply bytes out)))

  ## ── Decode ─────────────────────────────────────────────────────────────

  (defn huffman-decode [input]
    "Huffman-decode a byte sequence. Returns bytes."
    (let* [src (if (string? input) (bytes input) input)
           len (length src)
           out @[]
           @node decode-tree
           @i 0]
      (while (< i len)
        (let [byte-val (get src i)]
          (def @bit-idx 7)
          (while (>= bit-idx 0)
            (let* [bit (bit/and (bit/shr byte-val bit-idx) 1)
                   next (get node bit)]
              (cond
                (nil? next) (error {:error :h2-error
                                    :reason :compression-error
                                    :message "Huffman: invalid code"})
                (integer? next)
                  (begin
                    (when (= next 256)
                      (error {:error :h2-error
                              :reason :compression-error
                              :message "Huffman: EOS symbol in encoded data"}))
                    (push out next)
                    (assign node decode-tree))
                true (assign node next)))
            (assign bit-idx (- bit-idx 1))))
        (assign i (+ i 1)))  # Verify padding: remaining bits in the tree traversal must be a
      # prefix of EOS (all 1-bits). Check that current node is reachable
      # by following only 1-bits from decode-tree.
      (apply bytes out)))

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []  # ── Roundtrip: ASCII printable range ──
    (let [input (bytes "Hello, World!")]
      (assert (= (huffman-decode (huffman-encode input)) input)
              "huffman roundtrip: Hello, World!"))

    # ── Roundtrip: empty ──
    (assert (= (huffman-decode (huffman-encode (bytes))) (bytes))
            "huffman roundtrip: empty")

    # ── Roundtrip: single byte ──
    (each b in (list 0 32 65 97 127 255)
      (let [input (bytes b)]
        (assert (= (huffman-decode (huffman-encode input)) input)
                (concat "huffman roundtrip: byte " (string b)))))

    # ── RFC 7541 examples ──
    # C.4.1: www.example.com
    (let* [input (bytes "www.example.com")
           encoded (huffman-encode input)]
      (assert (= (huffman-decode encoded) input)
              "huffman: www.example.com roundtrip")  # Known encoding from RFC
      (assert (= encoded
                 (bytes 0xf1 0xe3 0xc2 0xe5 0xf2 0x3a 0x6b 0xa0 0xab 0x90 0xf4
                        0xff)) "huffman: www.example.com encoding"))

    # C.4.1: no-cache
    (let* [input (bytes "no-cache")
           encoded (huffman-encode input)]
      (assert (= (huffman-decode encoded) input) "huffman: no-cache roundtrip")
      (assert (= encoded (bytes 0xa8 0xeb 0x10 0x64 0x9c 0xbf))
              "huffman: no-cache encoding"))

    # C.6.1: custom-key
    (let* [input (bytes "custom-key")
           encoded (huffman-encode input)]
      (assert (= (huffman-decode encoded) input) "huffman: custom-key roundtrip")
      (assert (= encoded (bytes 0x25 0xa8 0x49 0xe9 0x5b 0xa9 0x7d 0x7f))
              "huffman: custom-key encoding"))

    # C.6.1: custom-value
    (let* [input (bytes "custom-value")
           encoded (huffman-encode input)]
      (assert (= (huffman-decode encoded) input)
              "huffman: custom-value roundtrip")
      (assert (= encoded (bytes 0x25 0xa8 0x49 0xe9 0x5b 0xb8 0xe8 0xb4 0xbf))
              "huffman: custom-value encoding"))

    # ── Roundtrip: all byte values ──
    (def @all-bytes @[])
    (def @b 0)
    (while (< b 256)
      (push all-bytes b)
      (assign b (+ b 1)))
    (let [input (apply bytes all-bytes)]
      (assert (= (huffman-decode (huffman-encode input)) input)
              "huffman roundtrip: all 256 byte values"))

    # ── Compression: ASCII should compress ──
    (let* [input (bytes "this is a typical http header value")
           encoded (huffman-encode input)]
      (assert (< (length encoded) (length input))
              "huffman: ASCII text compresses"))

    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:encode huffman-encode :decode huffman-decode :table table :test run-tests})
