(elle/epoch 15)
# audited: 2026-10-21
# std/gun values: what a field may hold, the encoding and its order, and a link with its trait table.
# lib/gun.md

(include-file "gun/check.lisp")

(def gun ((import "std/gun")))
(def value ((import "std/gun/value")))

# identical? and not =: = makes 1 and 1.0 equal and -0.0 and 0.0 equal, so a
# decoder that turned 1.0 into 1, or -0.0 into 0.0, would pass under =.
(each v in [nil true false 0 -7 42 9223372036854775807 1.0 -0.0 0.1 1e300
            -2.5e-300 "" "a b:c" "émoji 👋🏽" "l:not-a-link" (bytes)
            (bytes 0 255 16) (value:link "users/alice")]
  (assert (value:valid? v) (string "valid: " v))
  (assert (identical? (value:decode (value:encode v)) v)
          (string "round-trips: " v)))

(assert (= (value:encode nil) "n:") "nil encodes as n:")
(assert (= (value:encode true) "t:") "true encodes as t:")
(assert (= (value:encode false) "f:") "false encodes as f:")
(assert (= (value:encode -7) "i:-7") "an integer encodes in decimal")
(assert (= (value:encode 1.5) "d:1.5") "a float encodes in decimal")
(assert (= (value:encode "hi") "s:hi") "a string encodes as itself")
(assert (= (value:encode (bytes 0 255)) "x:00ff") "bytes encode in hex")
(assert (= (value:encode (value:link "a/b")) "l:a/b") "a link encodes its soul")
(assert (not (= (value:encode 1) (value:encode 1.0)))
        "an integer and a float encode apart")

(let [l (value:link "users/alice")]
  (assert (value:link? l) "a link is a link")
  (assert (= (value:link-soul l) "users/alice") "a link names its soul")
  (assert (not (value:link? {:soul "users/alice"}))
          "a struct of the same shape is not a link")
  (assert (not (value:link? "users/alice")) "a soul is not a link"))

# A trait table maps each protocol to a struct of methods (docs/traits.md).
# A tag such as {:gun :link} passes link? and every other test in this file;
# only reading the table itself catches it.
(each l in [(value:link "users/alice") (value:decode "l:users/alice")]
  (let [t (traits l)
        methods (get t :Gun)]
    (assert (= (keys t) (list :Gun)) "a link's table names the :Gun protocol")
    (assert (struct? methods) "the protocol is a struct")
    (each [name m] in methods
      (assert (fn? m) (string "and its " name " is a method")))
    (assert (= ((get methods :kind) l) :link) ":kind answers :link")
    (assert (= ((get methods :soul) l) "users/alice")
            ":soul answers the link's soul")))

(each v in [[1] @[1] (list 1) |1| {:a 1} @{:a 1} (fn [] 1) :keyword 'symbol
            (/ 0.0 0.0) (/ 1.0 0.0) (/ -1.0 0.0) @"mutable" (@bytes 1)]
  (assert (not (value:valid? v)) (string "not a value: " v)))

(each enc in ["" "q:1" "i:" "i:x" "d:nope" "x:0" "x:zz" "t:yes"]
  (let [[ok? err] (protect (value:decode enc))]
    (assert (not ok?) (string "does not decode: " enc))
    (assert (= (get err :reason) :bad-encoding)
            "a bad encoding is :bad-encoding")))

(let [samples [nil true false -1 0 1 2.5 "" "a" "b" (bytes 1) (value:link "s")]]
  (each a in samples
    (each b in samples
      (let [ab (value:before? a b)
            ba (value:before? b a)
            same (= (value:encode a) (value:encode b))]
        (assert (= 1 (+ (if ab 1 0) (if ba 1 0) (if same 1 0)))
                "exactly one of a before b, b before a, or the same encoding")))))

(assert (= (gun:link-soul (gun:link "a/b")) "a/b")
        "the module exports the link functions")

(all-passed "gun-value")
