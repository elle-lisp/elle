(elle/epoch 18)
# audited: 2026-11-24
# The instruction kinds glean knows: the fields of each that hold registers, and which kinds it marks or skips.
# tools/glean/overview.md
# tools/glean/present.md
# docs/spec/form.md

(fn []
  (def src-kinds
    [:is-nil :is-pair :is-array :is-array-mut :is-struct :is-struct-mut :is-set
     :is-set-mut :array-mut-len :match-fail :first-destructure :rest-destructure
     :first-or-nil :rest-or-nil :is-empty :is-bool :is-int :is-float :is-string
     :is-keyword :is-symbol-check :is-bytes :is-box :is-closure :is-fiber
     :type-of :length :pop :unary-op :convert :check-signal-bound :freeze :thaw
     :array-mut-ref-destructure :array-mut-slice-from :array-mut-ref-or-nil
     :struct-get-or-nil :struct-get-destructure :struct-rest :store-capture])

  # The fields of each kind that hold a register or an array of registers.
  # A StoreLocal names a value and uses none, so it lists no field.
  (def registers
    (merge (from-pairs (map (fn [k] [k [:src]]) src-kinds))
           {:const []
            :value-const []
            :materialize-const []
            :load-local []
            :store-local []
            :load-capture []
            :load-capture-raw []
            :load-self []
            :load-resume-value []
            :make-closure [:captures]
            :call [:func :args]
            :suspending-call [:func :args]
            :tail-call [:func :args]
            :call-array-mut [:func :args]
            :tail-call-array-mut [:func :args]
            :list [:head :tail]
            :make-array-mut [:elements]
            :first [:pair]
            :rest [:pair]
            :bin-op [:lhs :rhs]
            :compare [:lhs :rhs]
            :identical [:lhs :rhs]
            :make-capture-cell [:value]
            :load-capture-cell [:cell]
            :store-capture-cell [:cell :value]
            :eval [:expr :env]
            :array-mut-extend [:array :source]
            :array-mut-push [:array :value]
            :intr-push [:array :value]
            :intr-string-push [:string :value]
            :intr-bytes-push [:bytes :value]
            :get [:obj :key]
            :del [:obj :key]
            :has [:obj :key]
            :put [:obj :key :val]
            :push-param-frame [:pairs]
            :pop-param-frame []
            :return [:value]
            :jump []
            :branch [:cond]
            :emit [:value]
            :unreachable []}))

  # The present release protocol: no fact and no use.
  (def counts
    |:incref-region :decref-region :incref-value-region :decref-value-region
     :decref-cell-region :decref-postponed :adopt-region :adopt-cell-region
     :adopt-into-activation :free-region-group :assert-region-matches|)

  # The kinds with no rule, which mark their function.
  (def marked
    |:make-capture-cell :load-capture-cell :store-capture-cell :store-capture
     :array-mut-push :array-mut-extend :array-mut-slice-from :struct-rest :get
     :put :del :pop :intr-push :intr-string-push :intr-bytes-push :freeze :thaw
     :call-array-mut :tail-call-array-mut :push-param-frame :pop-param-frame
     :load-self|)

  # The kinds whose result is a computed immediate, which allocates nothing.
  (def computed
    |:bin-op :unary-op :convert :compare :identical :has :length :array-mut-len
     :type-of :is-nil :is-pair :is-array :is-array-mut :is-struct :is-struct-mut
     :is-set :is-set-mut :is-empty :is-bool :is-int :is-float :is-string
     :is-keyword :is-symbol-check :is-bytes :is-box :is-closure :is-fiber|)

  # The kinds that read a value out of a container, and the container's field.
  (def reads
    {:first :pair
     :rest :pair
     :first-or-nil :src
     :rest-or-nil :src
     :first-destructure :src
     :rest-destructure :src
     :array-mut-ref-destructure :src
     :array-mut-ref-or-nil :src
     :struct-get-or-nil :src
     :struct-get-destructure :src})

  (defn used [node]
    "Each register NODE reads, in field order. A count instruction reads none
  that glean sees. An unknown kind is an error."
    (def kind (get node :kind))
    (cond
      (has? counts kind) []
      (has? registers kind)
        (mapcat (fn [field]
                  (def held (get node field))
                  (if (array? held) held [held])) (get registers kind))
      (error {:error :glean-error
              :reason :unknown-kind
              :kind kind
              :message (string "glean: no rule knows the kind " kind)})))

  (defn known? [kind]
    "Whether KIND is a kind of the data form glean knows."
    (or (has? counts kind) (has? registers kind)))

  (defn immediate? [value]
    "Whether VALUE is a constant immediate: nil, the empty list, a boolean, a
  number, a symbol or a keyword."
    (match (type-of value)
      :nil true
      :boolean true
      :integer true
      :float true
      :symbol true
      :keyword true
      :list (empty? value)
      _ false))

  (defn immediate-name? [v]
    "Whether V is the name [:imm p] of an immediate."
    (and (array? v) (= (get v 0) :imm)))

  {:used used
   :known? known?
   :immediate? immediate?
   :immediate-name? immediate-name?
   :counts counts
   :marked marked
   :computed computed
   :reads reads})
