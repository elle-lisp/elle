(elle/epoch 15)
# audited: 2026-10-20
## string over bytes decodes UTF-8 into an immutable string and refuses bytes that are not UTF-8.
## docs/bytes.md
##
## The trap: an invalid byte, and a sequence cut short, are both refused as
## `:encoding-error` rather than decoded with a replacement character.

(assert (= (string (bytes 97 98 99)) "abc") "UTF-8 decode")
(assert (= (string (slice (bytes "xhello") 1)) "hello")
        "string over a slice of bytes")
(assert (= (type-of (string (bytes 97))) :string)
        "immutable bytes decode to a string")
(assert (= (string (@bytes 104 105)) "hi") "@bytes decode too")
(assert (= (string (bytes "héllo")) "héllo") "a multi-byte sequence decodes")
(assert (= (string (bytes)) "") "empty bytes decode to the empty string")
(println "  1. decoded")

(let [[ok? err] (protect (string (bytes 255)))]
  (assert (not ok?) "a byte that is not UTF-8 does not decode")
  (assert (= (get err :error) :encoding-error)
          "and the error names the encoding"))
(let [[ok? err] (protect (string (bytes 0xe2 0x82)))]
  (assert (not ok?) "a truncated sequence does not decode")
  (assert (= (get err :error) :encoding-error)))
(let [[ok? err] (protect (string (slice (bytes 97 0xe2 0x82 0xac) 0 3)))]
  (assert (not ok?) "a slice that cuts a sequence short does not decode")
  (assert (= (get err :error) :encoding-error)))
(println "  2. refused")

(println "bytes-string: ok")
