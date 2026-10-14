(elle/epoch 15)
# audited: 2026-10-14
## What a gun value may be, the link that points at a node, and the encoding that stores and orders values.
## lib/gun.md
## lib/gun/overview.md
##
## Loaded via:
##   (def value ((import "std/gun/value")))
##
## Exports: {:valid? :link :link? :link-soul :encode :decode :before?}
##
## A link is a struct {:soul s} whose trait table carries the :Gun protocol:
## :kind answers :link, and :soul answers s. Equality ignores traits, so
## link? reads the protocol, never the shape.

(fn []

  (def link-traits
    {:Gun {:kind (fn [self] :link) :soul (fn [self] (get self :soul))}})

  (defn kind-of [v]
    "What the :kind method of v's :Gun protocol answers, or nil when v
     carries no such protocol."
    (when-let [t (when (struct? v) (traits v))]
              (when-let [methods (get t :Gun)]
                        (let [kind (get methods :kind)]
                          (kind v)))))

  (defn link? [v]
    "True when v is a link built by link."
    (= (kind-of v) :link))

  (defn link [soul]
    "A link to the node named soul, a non-empty immutable string."
    (unless (and (= (type-of soul) :string) (not (empty? soul)))
      (error {:error :gun-error
              :reason :invalid-soul
              :soul soul
              :message "a soul is a non-empty string"}))
    (with-traits {:soul soul} link-traits))

  (defn link-soul [l]
    "The soul that link l names."
    (unless (link? l)
      (error {:error :gun-error
              :reason :not-a-link
              :value l
              :message "not a link"}))
    (get l :soul))

  (defn valid? [v]
    "True when v can be the value of a field."
    (cond
      (nil? v) true
      (boolean? v) true
      (integer? v) true
      (float? v)
        (not (or (nan? v) (inf? v)))
      (= (type-of v) :string) true
      (= (type-of v) :bytes) true
      (link? v)))

  (defn encode [v]
    "The text form of value v: a tag, a colon, and the payload."
    (cond
      (nil? v) "n:"
      (boolean? v) (if v "t:" "f:")
      (not (valid? v)) (error {:error :gun-error
                               :reason :invalid-value
                               :value v
                               :message "not a gun value"})
      (integer? v) (string "i:" v)
      (float? v) (string "d:" v)
      (string? v) (string "s:" v)
      (bytes? v) (string "x:" (seq->hex v))
      (string "l:" (link-soul v))))

  (defn bad-encoding [enc]
    (error {:error :gun-error
            :reason :bad-encoding
            :encoding enc
            :message (string "not a gun encoding: " enc)}))

  (defn canonical [enc payload parser]
    "The value parser reads from payload, when it writes back as payload."
    (let [[ok? v] (protect (parser payload))]
      (if (and ok? (= (string v) payload)) v (bad-encoding enc))))

  (defn decode-hex [enc payload]
    (let [n (length payload)
          out (@bytes)]
      (unless (even? n) (bad-encoding enc))
      (def @i 0)
      (while (< i n)
        (let [[ok? b] (protect (parse-int (slice payload i (+ i 2)) 16))]
          (unless (and ok? (>= b 0)) (bad-encoding enc))
          (push out b))
        (assign i (+ i 2)))
      (let [b (freeze out)]
        (unless (= (seq->hex b) payload) (bad-encoding enc))
        b)))

  (defn decode [enc]
    "The value that encode wrote as enc. Raises :bad-encoding on anything else."
    (unless (and (string? enc) (>= (length enc) 2) (= (get enc 1) ":"))
      (bad-encoding enc))
    (let [tag (get enc 0)
          payload (slice enc 2)
          bare (fn [v] (if (empty? payload) v (bad-encoding enc)))]
      (case tag
        "n" (bare nil)
        "t" (bare true)
        "f" (bare false)
        "i" (canonical enc payload parse-int)
        "d"
          (let [v (canonical enc payload parse-float)]
            (if (float? v) v (bad-encoding enc)))
        "s" payload
        "x" (decode-hex enc payload)
        "l" (if (empty? payload) (bad-encoding enc) (link payload))
        (bad-encoding enc))))

  (defn before? [a b]
    "True when the encoding of value a sorts before the encoding of b."
    (< (encode a) (encode b)))

  {:valid? valid?
   :link link
   :link? link?
   :link-soul link-soul
   :encode encode
   :decode decode
   :before? before?})
