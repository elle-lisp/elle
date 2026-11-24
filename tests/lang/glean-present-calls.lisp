(elle/epoch 18)
# audited: 2026-11-24
# A closure callee takes the references a call hands it, and a native callee takes none.
# tools/glean/present.md
#
# The counter-factual: a reader that ignored the moves would release a behind
# k1's call, and one that applied them to a native would hand a to length in
# k2. One that took one arm for a callee it cannot name would miss a path in
# k3. One that walked a closure's tail call on would release x and a behind it
# in k4, and one that ended a native's would hold a in k5. A late mint read as
# a hand-over would find k6's capture over. A callee or an arena whose release
# the new activation takes over would stay held in k7 and k8, and a spliced
# call's argument array in k9.

(include-file "glean/helpers.lisp")
(def present (get ((import "../../tools/glean/present")) :present))

(defn tail [dst func args &named fields]
  "A TailCall of FUNC with ARGS, its other fields as the lowerer writes them for
   a call with no borrowed argument, and FIELDS over them."
  (merge {:kind :tail-call
          :dst dst
          :func func
          :args args
          :arity-checked true
          :region 8
          :late-mints []
          :borrowed-arg-slots []
          :deferred-release-slot nil
          :defer-callee-release false} (or fields {})))

(def closure (fn [& xs] xs))

(defn moving [callee fields]
  "A function that moves a to CALLEE at 4 and releases a and the result r
   behind the call, FIELDS over it."
  (function (merge {:slots ['a 'r]
                    :blocks [(basic-block 0
                               [callee  # 0
                                (fresh-array 1 3)  # 1  a
                                (store-slot 0 1)  # 2
                                (load-slot 2 0)  # 3
                                (merge (invoke :call 3 0 [2])
                                       {:region 7 :moves [[0 0]]})  # 4  r
                                (store-slot 1 3)  # 5
                                (load-slot 4 0)  # 6
                                (release-value 4)  # 7
                                (load-slot 5 1)  # 8
                                (release-value 5)  # 9
                                (nil-const 6)]  # 10
                               (return-of 6))]}  # 11
                   fields)))

(def k1 (moving {:kind :value-const :dst 0 :value closure} {}))
(expect-present (present [k1]) 0 {:handed [['a 4]] :released [['r 9]]})

(def k2 (moving {:kind :value-const :dst 0 :value length} {}))
(expect-present (present [k2]) 0 {:released [['a 7] ['r 9]]})

(def k3 (moving {:kind :load-capture :dst 0 :index 0} {:captures ['f]}))
(expect-present (present [k3]) 0 {:handed [['a 4]] :released [['a 7] ['r 9]]})

# A closure's tail call hands each argument on, and leaves b held at its exit.
(def k4
  (function {:params 1
             :arity [1 1]
             :slots ['x 'a 'b]
             :blocks [(basic-block 0
                                   [{:kind :value-const :dst 0 :value closure}  # 0
                                    (fresh-array 1 3)  # 1  a
                                    (store-slot 1 1)  # 2
                                    (fresh-array 2 4)  # 3  b
                                    (store-slot 2 2)  # 4
                                    (load-slot 3 0)  # 5  x
                                    (load-slot 4 1)  # 6  a
                                    (tail 5 0 [3 4])]  # 7
                                   (return-of 5))]}))  # 8
(expect-present (present [k4]) 0 {:handed [['x 7] ['a 7]] :held [['b 7]]})

# A native's tail call falls through to the release of a and the Return.
(def k5
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [{:kind :value-const :dst 0 :value length}  # 0
                                    (fresh-array 1 3)  # 1  a
                                    (store-slot 0 1)  # 2
                                    (load-slot 2 0)  # 3
                                    (tail 3 0 [2])  # 4
                                    (load-slot 4 0)  # 5
                                    (release-value 4)]  # 6
                                   (return-of 3))]}))  # 7
(expect-present (present [k5]) 0 {:released [['a 6]] :handed [[[:reg 3] 7]]})

# The runtime mints the capture's reference at the call, so k hands on nothing.
(def k6
  (function {:captures ['k]
             :slots [nil]
             :blocks [(basic-block 0
                                   [{:kind :value-const :dst 0 :value closure}  # 0
                                    {:kind :load-capture :dst 1 :index 0}  # 1  k
                                    (tail 2 0 [1]
                                      :fields {:late-mints [[0 0]]
                                               :borrowed-arg-slots [0]})]  # 2
                                   (return-of 2))]}))  # 3
(expect-present (present [k6]) 0 {})

# The new activation releases the callee f on the closure arm, and the native
# arm releases it behind the call.
(def k7
  (function {:slots ['f]
             :blocks [(basic-block 0
                                   [{:kind :make-closure
                                     :dst 0
                                     :closure-id 1
                                     :captures []
                                     :region 4}  # 0  f
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (tail 2 1 []
                                      :fields {:defer-callee-release true})  # 3
                                    (load-slot 3 0)  # 4
                                    (release-value 3)]  # 5
                                   (return-of 2))]}))  # 6
(expect-present (present [k7]) 0
                {:released [['f 3] ['f 5]] :handed [[[:reg 2] 6]]})

# The new activation releases the arena static region 5 maps.
(def k8
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [{:kind :value-const :dst 0 :value closure}  # 0
                                    (fresh-array 1 5)  # 1  a
                                    (store-slot 0 1)  # 2
                                    (tail 2 0 []
                                      :fields {:deferred-release-slot 5})]  # 3
                                   (return-of 2))]}))  # 4
(expect-present (present [k8]) 0 {:released [['a 3]]})

# A spliced call releases its argument array, which static region 6 maps.
(def k9
  (function {:slots [nil]
             :blocks [(basic-block 0
                                   [{:kind :value-const :dst 0 :value closure}  # 0
                                    (fresh-array 1 6)  # 1
                                    {:kind :call-array-mut
                                     :dst 2
                                     :func 0
                                     :args 1
                                     :region 7
                                     :args-region 6}]  # 2
                                   (return-of 2))]}))  # 3
(expect-present (present [k9]) 0
                {:released [[[:reg 1] 2]] :handed [[[:reg 2] 3]]})

(println "glean-present-calls: ok")
