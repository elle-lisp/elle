(elle/epoch 14)
# audited: 2026-10-06
# The pages one HPACK Huffman decode claims, for a value of short codes and a value of long ones.
# lib/http2/overview.md
#
# Each interpreted loop step claims pages, so the pages a decode claims
# track the loop steps it runs: one per bit for a decoder that walks the
# trie, one per code for a decoder that reads a table. The page gauge is
# deterministic, so a reading is exact and the ledger pins it in
# tests/ledger/huffman-decode.lisp.
#
# Both values are 100 bytes before coding. The short value is letters,
# each a code of 5 to 7 bits; the long value is `\^~{}`, each a code of 13
# to 19 bits, which no byte-wide table holds. The long subject is the
# control: a table decode must not cost a long code more than the walk did.

(def r ((import "std/ratchet")))
(def huffman ((import "std/http2/huffman")))

(def short-codes (huffman:encode (bytes (string/repeat "abcdefghij" 10))))
(def long-codes (huffman:encode (bytes (string/repeat "\\^~{}" 20))))

(defn decode-rate [subject coded]
  (r:rate subject (fn [j] (huffman:decode coded)) :on [r:pages] :block 20 :min 4))

(decode-rate "decode 100 short codes" short-codes)
(decode-rate "decode 100 long codes" long-codes)

(println "huffman-decode: ok")
