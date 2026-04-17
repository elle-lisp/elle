# ── Bitwise op agreement across tiers ─────────────────────────────────
#
# Covers: BinOp::{BitAnd,BitOr,BitXor,Shl,Shr}.

(def diff ((import "std/differential")))

(defn band [a b] (bit/and a b))
(diff:assert-agree band 0xff 0x0f)
(diff:assert-agree band 0xaa 0x55)
(diff:assert-agree band -1 0xff)

(defn bor [a b] (bit/or a b))
(diff:assert-agree bor 0xf0 0x0f)
(diff:assert-agree bor 0 -1)
(diff:assert-agree bor 0x55 0xaa)

(defn bxor [a b] (bit/xor a b))
(diff:assert-agree bxor 0xff 0x0f)
(diff:assert-agree bxor 0xaa 0xaa)
(diff:assert-agree bxor 0 0xff)

(defn shl [a n] (bit/shl a n))
(diff:assert-agree shl 1 0)
(diff:assert-agree shl 1 4)
(diff:assert-agree shl 1 31)

(defn shr [a n] (bit/shr a n))
(diff:assert-agree shr 0xff 4)
(diff:assert-agree shr 1024 8)
(diff:assert-agree shr -16 2)

(println "bitwise: OK")
