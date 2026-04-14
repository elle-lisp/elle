## model.lisp — GPT model: initialization, forward pass, loss
##
## Architecture: GPT-2 style transformer (RMSNorm, no biases, ReLU
## instead of GeLU). Single-layer, 4-head attention with KV cache
## for incremental inference.

(fn [ag helpers rng]

  (def {:make-value make-value :v-data v-data :v-grad v-grad
        :v+ v+ :v* v* :vpow vpow :vexp vexp :vlog vlog :vrelu vrelu
        :v/ v/ :v*s v*s :v+s v+s :vneg vneg} ag)
  (def {:make-2d make-2d :make-kv-caches make-kv-caches} helpers)

  # ── Hyperparameters ────────────────────────────────────────────

  (def *n-embd* 16)
  (def *n-head* 4)
  (def *head-dim* 4)
  (def *n-layer* 1)
  (def *block-size* 16)
  (def *mlp-hidden* 64)
  (def *eps* 0.00000001)

  # ── Parameter initialization ───────────────────────────────────

  (defn init-weight [rows cols scale]
    "Create a rows x cols 2D array of Value nodes with Gaussian random init."
    (make-2d rows cols
      (fn [r c] (make-value (rng:normal 0.0 scale)))))

  (defn layer-key [i suffix]
    (string/format "layer{}.{}" i suffix))

  (defn init-model [vocab-size]
    "Initialize all model parameters. Returns an @struct of named weight matrices."
    (let* ([scale (/ 1.0 (sqrt (float *n-embd*)))]
           [model @{:wte (init-weight vocab-size *n-embd* scale)
                    :wpe (init-weight *block-size* *n-embd* scale)
                    :lm-head (init-weight vocab-size *n-embd* scale)}])
      (each layer in (range *n-layer*)
        (put model (layer-key layer "attn-wq") (init-weight *n-embd* *n-embd* scale))
        (put model (layer-key layer "attn-wk") (init-weight *n-embd* *n-embd* scale))
        (put model (layer-key layer "attn-wv") (init-weight *n-embd* *n-embd* scale))
        (put model (layer-key layer "attn-wo") (init-weight *n-embd* *n-embd* scale))
        (put model (layer-key layer "mlp-fc1") (init-weight *mlp-hidden* *n-embd* scale))
        (put model (layer-key layer "mlp-fc2") (init-weight *n-embd* *mlp-hidden* scale)))
      model))

  # ── Collect parameters ─────────────────────────────────────────

  (defn collect-params [model]
    "Collect all Value parameter nodes from the model into a flat array."
    (let* ([params @[]])
      (each key in (keys model)
        (each row in (model key)
          (each val in row
            (push params val))))
      params))

  # ── Forward pass building blocks ───────────────────────────────

  (defn mat-vec-mul [mat vec-in]
    "Matrix-vector multiply: mat (2D array of Values) x vec-in (1D array of Values)."
    (let* ([result @[]])
      (each row in mat
        (let* ([acc (make-value 0.0)])
          (var c 0)
          (while (< c (length row))
            (assign acc (v+ acc (v* (row c) (vec-in c))))
            (assign c (inc c)))
          (push result acc)))
      result))

  (defn vec-add [a b]
    "Element-wise autograd addition of two vectors."
    (let* ([result @[]])
      (var i 0)
      (while (< i (length a))
        (push result (v+ (a i) (b i)))
        (assign i (inc i)))
      result))

  (defn rms-norm [vec-in]
    "RMS normalization: x / sqrt(mean(x^2) + eps)."
    (let* ([n (length vec-in)]
           [sum-sq (make-value 0.0)])
      (each v in vec-in
        (assign sum-sq (v+ sum-sq (v* v v))))
      (let* ([mean-sq (v*s sum-sq (/ 1.0 n))]
             [rms (vpow (v+s mean-sq *eps*) 0.5)])
        (thaw (->array (map (fn [v] (v/ v rms)) vec-in))))))

  (defn softmax-values [scores]
    "Softmax over an array of Value nodes. Returns array of Value nodes."
    (var max-val (v-data (scores 0)))
    (each s in scores
      (let* ([d (v-data s)])
        (when (> d max-val) (assign max-val d))))
    (let* ([exps @[]]
           [sum-exp (make-value 0.0)])
      (each s in scores
        (let* ([e (vexp (v+s s (- 0.0 max-val)))])
          (push exps e)
          (assign sum-exp (v+ sum-exp e))))
      (thaw (->array (map (fn [e] (v/ e sum-exp)) exps)))))

  (defn layer-weights [model i]
    "Get all weight matrices for transformer layer i."
    {:wq  (model (layer-key i "attn-wq"))
     :wk  (model (layer-key i "attn-wk"))
     :wv  (model (layer-key i "attn-wv"))
     :wo  (model (layer-key i "attn-wo"))
     :fc1 (model (layer-key i "mlp-fc1"))
     :fc2 (model (layer-key i "mlp-fc2"))})

  # ── Per-token forward pass ─────────────────────────────────────

  (defn attn-head [h q layer-keys layer-vals n-t x-attn]
    "Compute one attention head and write results into x-attn."
    (let* ([hs (* h *head-dim*)]
           [q-head (slice q hs (+ hs *head-dim*))]
           [sf (/ 1.0 (sqrt (float *head-dim*)))]
           [attn-logits @[]])
      (var ti 0)
      (while (< ti n-t)
        (let* ([k-t (layer-keys ti)]
               [dot (make-value 0.0)])
          (var d 0)
          (while (< d *head-dim*)
            (assign dot (v+ dot (v* (q-head d) (k-t (+ hs d)))))
            (assign d (inc d)))
          (push attn-logits (v*s dot sf)))
        (assign ti (inc ti)))
      (let* ([aw (softmax-values attn-logits)])
        (each j in (range *head-dim*)
          (let* ([acc (make-value 0.0)])
            (var t2 0)
            (while (< t2 n-t)
              (assign acc (v+ acc (v* (aw t2)
                                      ((layer-vals t2) (+ hs j)))))
              (assign t2 (inc t2)))
            (put x-attn (+ hs j) acc))))))

  (defn attn-block [x weights kv-keys kv-values li]
    "Multi-head attention with KV cache. Returns updated x with residual."
    (let* ([x-norm (rms-norm x)]
           [q (mat-vec-mul weights:wq x-norm)]
           [k (mat-vec-mul weights:wk x-norm)]
           [v (mat-vec-mul weights:wv x-norm)]
           [layer-keys (begin (push (kv-keys li) k) (kv-keys li))]
           [layer-vals (begin (push (kv-values li) v) (kv-values li))]
           [n-t (length layer-keys)]
           [x-attn (array/new *n-embd* (make-value 0.0))])
      (each h in (range *n-head*)
        (attn-head h q layer-keys layer-vals n-t x-attn))
      (vec-add (mat-vec-mul weights:wo x-attn) x)))

  (defn mlp-block [x weights]
    "MLP block with residual connection."
    (let* ([h (mat-vec-mul weights:fc1 (rms-norm x))]
           [h (thaw (->array (map vrelu h)))]
           [h (mat-vec-mul weights:fc2 h)])
      (vec-add h x)))

  (defn gpt-forward-token [token-id pos-id kv-keys kv-values model]
    "Forward pass for a single token at position pos-id.
     Mutates kv-keys and kv-values by appending new k/v vectors.
     Returns a 1D array of logit Value nodes."
    (var x (rms-norm (vec-add (model:wte token-id) (model:wpe pos-id))))
    (each li in (range *n-layer*)
      (let* ([weights (layer-weights model li)])
        (assign x (attn-block x weights kv-keys kv-values li))
        (assign x (mlp-block x weights))))
    (mat-vec-mul model:lm-head x))

  # ── Loss ───────────────────────────────────────────────────────

  (defn cross-entropy-loss-incremental [model tokens]
    "Compute cross-entropy loss over a token sequence using incremental forward.
     Returns a single Value node (the mean loss)."
    (let* ([n (min *block-size* (dec (length tokens)))]
           [[kv-keys kv-values] (make-kv-caches *n-layer*)]
           [total-loss (make-value 0.0)])
      (var pos 0)
      (while (< pos n)
        (let* ([token-id (tokens pos)]
               [target-id (tokens (inc pos))]
               [logits (gpt-forward-token token-id pos kv-keys kv-values model)]
               [probs (softmax-values logits)]
               [loss-t (vneg (vlog (probs target-id)))])
          (assign total-loss (v+ total-loss loss-t)))
        (assign pos (inc pos)))
      (v*s total-loss (/ 1.0 (float n)))))

  {:init-model init-model :collect-params collect-params
   :gpt-forward-token gpt-forward-token
   :cross-entropy-loss-incremental cross-entropy-loss-incremental
   :*block-size* *block-size* :*n-layer* *n-layer*})
