(elle/epoch 14)
# audited: 2026-10-12
# std/gun's peer messages: each kind round-trips as JSON, a link stays a link, and a bad message is refused.
# lib/gun/mesh.md
# lib/gun/sea.md

(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))
(def ham ((import "std/gun/ham") :state state :value value))
(def graph ((import "std/gun/graph") :state state :value value :ham ham))
(def wire ((import "std/gun/wire") :value value :graph graph))

(defn fails-with [reason thunk]
  "Run thunk, and answer the error it raises after checking its reason."
  (let [[ok? err] (protect (thunk))]
    (assert (not ok?) (string "expected an error with reason " reason))
    (assert (= (get err :error) :gun-error) "a gun error")
    (assert (= (get err :reason) reason) (string "the reason is " reason))
    err))

(let [g {"users/alice" {"name" [7 "Alice"]
                        "boss" [7 (value:link "bob")]
                        "gone" [8 nil]}}]
  (each m in [(wire:hi "db1") (wire:put "m1" g) (wire:get "m2" ["a" "b"])
              (wire:answer "m2" g) (wire:answer "m3" {}) (wire:ack "m1")
              (wire:nack "m1" :boom)]
    (assert (= (wire:decode (wire:encode m)) m) "a message round-trips"))
  # = ignores traits, so the round trip above holds even when a link comes
  # back as a plain struct. Raw JSON of the value would do exactly that.
  (let [back (wire:decode (wire:encode (wire:put "m1" g)))
        boss (get (get (get back :graph) "users/alice") "boss")]
    (assert (value:link? (get boss 1)) "a link arrives as a link")))

(assert (= (wire:encode (wire:put "m1" {"s" {"k" [7 "x"]}}))
           "{\"id\":\"m1\",\"put\":{\"s\":{\"k\":[7,\"s:x\"]}}}")
        "a put carries each field as [state encoding]")
(assert (= (wire:encode (wire:nack "m1" :boom))
           "{\"ack\":\"m1\",\"err\":\"boom\"}")
        "a failed put acks err with the reason's name")

# A field in a user space carries its signature as a third element, in hex.
(let [sig-hex (string/repeat "ab" 64)
      sig (value:decode (string "x:" sig-hex))
      m (wire:put "m1" {"~s" {"k" [7 "x" sig]}})]
  (assert (= (wire:encode m)
             (string "{\"id\":\"m1\",\"put\":{\"~s\":{\"k\":[7,\"s:x\",\""
                     sig-hex "\"]}}}"))
          "a signed field travels as [state encoding signature]")
  (assert (= (wire:decode (wire:encode m)) m) "a signed field round-trips")
  (assert (bytes? (get (get (get (get (wire:decode (wire:encode m)) :graph) "~s")
                            "k") 2)) "its signature arrives as bytes"))

(each sig in [(string/repeat "AB" 64) (string/repeat "ab" 63) "" "zz"]
  (fails-with :bad-message (fn []
                             (wire:decode (string "{\"id\":\"m\",\"put\":{\"s\":{\"k\":[1,\"i:1\",\""
                             sig "\"]}}}")))))
(fails-with :bad-message (fn []
                           (wire:decode "{\"id\":\"m\",\"put\":{\"s\":{\"k\":[1,\"i:1\",7]}}}")))
(fails-with :bad-message (fn []
                           (wire:decode (string "{\"id\":\"m\",\"put\":{\"s\":{\"k\":[1,\"i:1\",\""
                                        (string/repeat "ab" 64) "\",1]}}}"))))

(each text in ["not json" "[1,2]" "{\"what\":1}" "{\"put\":{}}"
               "{\"id\":\"m\",\"put\":{\"s\":{\"k\":[1,\"q:zz\"]}}}"
               "{\"id\":\"m\",\"put\":{\"s\":{\"_\":[1,\"i:1\"]}}}"
               "{\"id\":\"m\",\"put\":{\"s\":{\"k\":[-1,\"i:1\"]}}}"
               "{\"id\":\"m\",\"put\":{\"s\":{\"k\":[1.5,\"i:1\"]}}}"
               "{\"id\":\"m\",\"put\":{\"s\":{\"k\":[1]}}}"
               "{\"id\":\"m\",\"put\":{\"\":{\"k\":[1,\"i:1\"]}}}"
               "{\"id\":\"m\",\"get\":[1]}" "{\"id\":\"m\",\"get\":\"s\"}"
               "{\"hi\":7}"]
  (fails-with :bad-message (fn [] (wire:decode text))))

(println "tests/lang/gun-wire.lisp: all tests passed")
