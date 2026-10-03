(elle/epoch 13)
# audited: 2026-09-23
# The string escapes: what each one epoch 13 reads stands for, which escapes
# fail to read, and the reading a source that declares epoch 12 keeps. Before
# epoch 13 an unknown escape read as the character after its backslash, so
# "\0" held the digit 0 and nothing said so.
# docs/syntax.md

# ── the escapes epoch 13 adds ───────────────────────────────────────────

(assert (= "\x41\x42" "AB") "a hex escape names an ASCII character")
(assert (= (string/size-of "\x00") 1) "a hex escape is one byte")
(assert (= (length "\0") 1) "NUL is one character")
(assert (= "\0" "\x00") "NUL is \\x00")
(assert (= "\0" "\u{0}") "NUL is \\u{0}")
(assert (not (has? "10" "\0")) "the digit 0 is not NUL")
(assert (= "\u{e9}" "é") "a unicode escape names a scalar value")
(assert (= (string/size-of "\u{e9}") 2) "U+00E9 is two bytes of UTF-8")
(assert (= "\u{1F600}" "😀") "a unicode escape reaches past the BMP")
(assert (= (freeze @"\x41") "A") "a mutable string reads the same escapes")

# ── the five escapes every epoch reads ──────────────────────────────────

(assert (= (string/size-of "\n\t\r\\\"") 5)
        "each shared escape is one character")

# ── the escapes epoch 13 refuses ────────────────────────────────────────

(defn reads? [text]
  (let [[ok? _] (protect (read text))]
    ok?))

(assert (not (reads? "\"\\e\"")) "\\e is not an escape")
(assert (not (reads? "\"\\a\"")) "\\a is not an escape")
(assert (not (reads? "\"\\'\"")) "\\' is not an escape")
(assert (not (reads? "\"\\x80\"")) "\\x80 names no one byte and character")
(assert (not (reads? "\"\\x4\"")) "a hex escape takes two digits")
(assert (not (reads? "\"\\u{d800}\"")) "a surrogate is not a scalar value")
(assert (not (reads? "\"\\u{110000}\"")) "U+110000 is not a scalar value")
(assert (reads? "\"\\x7f\"") "\\x7f is the last hex escape")
(assert (reads? "\"\\u{10FFFF}\"") "U+10FFFF is the last scalar value")

# ── a source that declares epoch 12 keeps its reading ───────────────────

(assert (= (read-all "(elle/epoch 12) \"\\x41\\0\\u{e9}\\q\"")
           (list '(elle/epoch 12) "x410u{e9}q"))
        "epoch 12 drops the backslash of each escape epoch 13 added")
