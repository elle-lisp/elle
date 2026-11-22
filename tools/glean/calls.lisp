(elle/epoch 18)
# audited: 2026-11-22
# Glean's rule for a call: its callee's name, its arguments, and what a native's :spec says each argument does.
# tools/glean/overview.md
# docs/spec/signature.md

(fn []
  (defn add [facts key fact]
    "Append FACT to the tuples FACTS holds under KEY."
    (def held (get facts key))
    (if (nil? held) (put facts key @[fact]) (push held fact)))

  (defn mark [marks key]
    "Count one more unruled KEY, a kind or a native's name, in MARKS."
    (put marks key (+ 1 (get marks key 0))))

  (defn closure-index [closures closure]
    "CLOSURE's index in the unit's table CLOSURES, appended at first sight."
    (var k 0)
    (while (and (< k (length closures))
                (not (identical? (get closures k) closure)))
      (assign k (+ k 1)))
    (when (= k (length closures)) (push closures closure))
    k)

  (defn native-entry [natives native]
    "The compile/primitives entry whose :fn is NATIVE."
    (def hit (filter (fn [entry] (identical? (get entry :fn) native)) natives))
    (when (empty? hit)
      (error {:error :glean-error
              :reason :unknown-native
              :message "glean: compile/primitives names no such native"}))
    (first hit))

  (defn constant-fn [names r]
    "The function a ValueConst writes into register R, or nil."
    (def node (names:written r))
    (when (= (get node :kind) :value-const)
      (let [v (get node :value)]
        (when (fn? v) v))))

  (defn call-tuples [facts names d f args p]
    "call(D, F, P), and arg(y, i, P) for each argument y that names a value."
    (add facts :call [d f p])
    (each i in (range (length args))
      (let [y (names:name (get args i))]
        (unless (nil? y) (add facts :arg [y i p])))))

  (defn spec-facts [facts names spec d args p]
    "The facts a native's SPEC gives at P: resume and cross for a resumed or
  crossing argument, and payload for a payload result."
    (def params (get spec :params))
    (each i in (range (length args))
      (let [y (names:name (get args i))
            mode (if (< i (length params)) (get params i) (get spec :rest))]
        (unless (nil? y)
          (cond
            (= mode :resumed) (add facts :resume [y p])
            (= mode :crosses) (add facts :cross [y p])))))
    (def result (get spec :result))
    (when (and (array? result) (= (get result 0) :payload))
      (let [y (names:name (get args (get result 1)))]
        (unless (nil? y) (add facts :payload [d y p])))))

  (defn call-facts [ctx facts marks node p]
    "The facts of the call NODE at P. A call to a native with no :spec is
  unruled: it marks the function with the native's name and gives no fact
  here. Answers whether the call is ruled."
    (def names (get ctx :names))
    (def d (names:name (get node :dst)))
    (def args (get node :args))
    (def f (constant-fn names (get node :func)))
    (cond
      (native-fn? f)
        (let [entry (native-entry (get ctx :natives) f)
              spec (get entry :spec)]
          (if (nil? spec)
            (begin
              (mark marks (get entry :name))
              false)
            (begin
              (call-tuples facts names d (get entry :name) args p)
              (spec-facts facts names spec d args p)
              true)))
      (closure? f)
        (begin
          (call-tuples facts names d [:fn (closure-index (get ctx :closures) f)]
                       args p)
          true)
      (let [callee (names:name (get node :func))]
        (when (nil? callee)
          (error {:error :glean-error
                  :reason :no-callee
                  :point p
                  :message (string "glean: the call at " p " names no callee")}))
        (call-tuples facts names d callee args p)
        true)))

  {:add add :mark mark :call-facts call-facts})
