(elle/epoch 12)
# audited: 2026-09-29
# string/size-of measures a string in bytes, not graphemes.
# docs/strings.md
#

(assert (= (string/size-of "hello") 5) "size-of ASCII")
(assert (= (string/size-of "café") 5) "size-of multibyte UTF-8")
(assert (= (string/size-of "🎉") 4) "size-of emoji")
(assert (= (string/size-of "") 0) "size-of empty")

(let [[ok? _] (protect ((fn [] (string/size-of 42))))]
  (assert (not ok?) "size-of type error"))

(println "prim-string: all tests passed")
