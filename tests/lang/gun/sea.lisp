(elle/epoch 15)
# audited: 2026-10-21
# The SEA harness of the std/gun tests: seeds, hex, and the signature over a field that lib/gun/sea.md states.
# lib/gun/sea.md
# tests/lang/overview.md
#
# A file that includes this one binds crypto, the crypto plugin, and value,
# the std/gun/value module.

(defn seed-of [n &opt size]
  "Bytes of size, 32 by default, each of them n."
  (let [out (@bytes)]
    (each _ in (range (or size 32))
      (push out n))
    (freeze out)))

(defn hex->bytes [h]
  "The bytes that the hex text h spells."
  (value:decode (string "x:" h)))

(defn signed-text [soul key st v]
  "The text that a signature over the field key of soul covers."
  (json/serialize ["gun/sea/sign" soul key st (value:encode v)]))

(defn sign [seed soul key st v]
  "The hex signature, under the pair of seed, of the field key of soul."
  (seq->hex (crypto:ed25519-sign (crypto:hkdf-sha256 seed "" "gun/sea/sign" 32)
                                 (signed-text soul key st v))))

(defn verifies? [pair soul key f]
  "True when field f of key in soul carries the signature of pair."
  (let [[st v sig] f]
    (and (bytes? sig)
         (crypto:ed25519-verify (hex->bytes pair:pub)
                                (signed-text soul key st v) sig))))
