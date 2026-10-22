(elle/epoch 15)
# audited: 2026-10-21
## The peer messages of std/gun: build them, write them as JSON text, and read them back with every check.
## lib/gun/mesh.md
## lib/gun/overview.md
##
## Loaded via:
##   (def wire ((import "std/gun/wire") :value value :graph graph))
##
## Exports: {:hi :put :get :answer :ack :nack :encode :decode}
##
## A message is a struct with a :kind. A graph in a message maps each soul to
## its fields, a struct of key to field; on the wire each value travels in
## its value:encode form, which keeps a link a link, and a signature travels
## in hex. Decoding checks a signature's form, never whether it verifies.

(fn [&named value graph]

  (def field ((import "std/gun/field")))

  ## ── Building ─────────────────────────────────────────────────────────

  (defn hi [id]
    "The first message on a connection: the database id of its sender."
    {:kind :hi :peer id})

  (defn put-message [id g]
    "A put of the graph g, under the message id id."
    {:kind :put :id id :graph g})

  (defn get-message [id souls]
    "A get of the whole nodes souls names, under the message id id."
    {:kind :get :id id :souls souls})

  (defn answer [ack g]
    "The answer to the get ack: the graph g of the nodes this side holds."
    {:kind :answer :ack ack :graph g})

  (defn ack [id]
    "The answer to the put id: its merge went through."
    {:kind :ack :ack id :ok? true})

  (defn nack [id reason]
    "The answer to the put id: its merge raised an error whose reason was
     the keyword reason."
    {:kind :ack :ack id :ok? false :reason reason})

  ## ── Writing ──────────────────────────────────────────────────────────

  (defn graph->json [g]
    (let [out @{}]
      (each [soul fields] in g
        (let [node @{}]
          (each [key f] in fields
            (let [st (field:state f)
                  enc (value:encode (field:value f))
                  sig (field:signature f)]
              (put node key (if sig [st enc (seq->hex sig)] [st enc]))))
          (put out soul (freeze node))))
      (freeze out)))

  (defn encode [m]
    "The JSON text of message m."
    (json/serialize (case m:kind
                      :hi {"hi" m:peer}
                      :put {"id" m:id "put" (graph->json m:graph)}
                      :get {"id" m:id "get" m:souls}
                      :answer {"ack" m:ack "put" (graph->json m:graph)}
                      :ack
                        (if m:ok?
                          {"ack" m:ack "ok" true}
                          {"ack" m:ack "err" (string m:reason)}))))

  ## ── Reading ──────────────────────────────────────────────────────────

  (defn bad [why text]
    (error {:error :gun-error
            :reason :bad-message
            :why why
            :text text
            :message (string "not a gun message: " why)}))

  (defn soul? [s]
    (and (= (type-of s) :string) (not (empty? s))))

  (defn checked [why text thunk]
    "What thunk answers, or a :bad-message error naming why when it raises."
    (let [[ok? v] (protect (thunk))]
      (if ok? v (bad why text))))

  (defn json->signature [sig text]
    "The bytes that sig, 128 lowercase hex characters, spells."
    (unless (and (= (type-of sig) :string) (= (length sig) 128))
      (bad "a signature is not 128 hex characters" text))
    (checked "a signature is not lowercase hex" text
             (fn [] (value:decode (string "x:" sig)))))

  (defn json->field [f text]
    (unless (and (array? f) (or (= (length f) 2) (= (length f) 3)))
      (bad "a field is not [state encoding] or [state encoding signature]" text))
    (let [[st enc] f]
      (unless (and (integer? st) (>= st 0))
        (bad "a state is not an integer of zero or more" text))
      (unless (= (type-of enc) :string) (bad "an encoding is not a string" text))
      (field:make st
                  (checked "a value does not decode" text
                           (fn [] (value:decode enc)))
                  (when (= (length f) 3) (json->signature (get f 2) text)))))

  (defn json->graph [g text]
    (unless (struct? g) (bad "a graph is not an object" text))
    (let [out @{}]
      (each [soul node] in g
        (unless (soul? soul) (bad "a soul is not a non-empty string" text))
        (unless (struct? node) (bad "a node is not an object" text))
        (let [fields @{}]
          (each [key f] in node
            (checked "a key is not a gun key" text (fn [] (graph:key-of key)))
            (put fields key (json->field f text)))
          (put out soul (freeze fields))))
      (freeze out)))

  (defn message-id [m key text]
    (let [id (get m key)]
      (unless (soul? id) (bad (string "\"" key "\" is not a message id") text))
      id))

  (defn decode [text]
    "The message that encode wrote as text. Raises :bad-message on anything
     else; lib/gun/mesh.md lists what a message may hold."
    (let [[ok? m] (protect (json/parse text))]
      (unless (and ok? (struct? m)) (bad "not a JSON object" text))
      (cond
        (has-key? m "hi")
          (let [id (get m "hi")]
            (unless (soul? id) (bad "\"hi\" is not a database id" text))
            (hi id))
        (has-key? m "ack")
          (let [id (message-id m "ack" text)]
            (cond
              (has-key? m "put")
                (answer id (json->graph (get m "put") text))
              (= (get m "ok") true) (ack id)
              (= (type-of (get m "err")) :string)
                (nack id (keyword (get m "err")))
              (bad "an ack holds no put, ok or err" text)))
        (has-key? m "put")
          (put-message (message-id m "id" text) (json->graph (get m "put") text))
        (has-key? m "get")
          (let [id (message-id m "id" text)
                souls (get m "get")]
            (unless (and (array? souls) (all? soul? souls))
              (bad "\"get\" is not a list of souls" text))
            (get-message id souls))
        (bad "no known message kind" text))))

  {:hi hi
   :put put-message
   :get get-message
   :answer answer
   :ack ack
   :nack nack
   :encode encode
   :decode decode})
